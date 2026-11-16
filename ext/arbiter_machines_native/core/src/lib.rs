#![no_std]
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]
#![doc = include_str!("../README.md")]

use core::fmt;

/// Resource priority protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// Transitive inheritance from blocked waiters.
    Inheritance,
    /// Immediate ceilings plus system-ceiling admission.
    ImmediateCeiling,
}

/// Honoured answer to [`Arbiter::acquire`]; no underlying lock is manipulated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Acquisition {
    /// The task now owns the resource. A task that asks is running, not
    /// waiting, so a wait it had recorded on another resource is cleared; the
    /// caller learns that wait from the `Blocked` it was given, or from
    /// [`Arbiter::waiting_on`] before the call.
    Granted,
    /// The task already owns the resource: an idempotent repeat that changed
    /// nothing. Recursion depth is not tracked.
    AlreadyOwned,
    /// The task must wait, and its wait on this blocking resource index is now
    /// recorded (replacing any earlier wait). Asking again while the same
    /// resource still blocks answers the same and changes nothing.
    Blocked(usize),
}

/// Honoured answer to [`Arbiter::cancel_wait`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cancelling {
    /// The task's recorded wait was dropped.
    Cancelled,
    /// The task had no wait recorded: an idempotent repeat that changed nothing.
    NotWaiting,
}

/// Why a call was not honoured. Every refusal leaves the arbiter exactly as
/// it was: no owner, no wait moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The task index is not one of the arbiter's `T` tasks.
    UnknownTask,
    /// The resource index is not one of the arbiter's `R` resources.
    UnknownResource,
    /// Under [`Protocol::ImmediateCeiling`], the task's base priority is above
    /// the resource's declared ceiling: a misdeclaration.
    AboveCeiling,
    /// Recording this wait would close a wait-for cycle.
    Deadlock,
    /// Nobody owns the resource. A repeated release lands here: the arbiter
    /// keeps no history, so it cannot prove the task ever owned it.
    Unowned,
    /// Another task owns the resource.
    WrongOwner,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::UnknownTask => "no such task",
            Self::UnknownResource => "no such resource",
            Self::AboveCeiling => "task priority is above the resource ceiling",
            Self::Deadlock => "waiting would close a wait-for cycle",
            Self::Unowned => "nobody owns the resource",
            Self::WrongOwner => "another task owns the resource",
        })
    }
}

impl core::error::Error for Refusal {}

/// Fixed task/resource ownership and wait-for graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arbiter<const T: usize, const R: usize> {
    base: [u32; T],
    ceilings: [u32; R],
    protocol: Protocol,
    owners: [Option<usize>; R],
    waiting: [Option<usize>; T],
}

impl<const T: usize, const R: usize> Arbiter<T, R> {
    /// Configure base priorities and resource ceilings. Nothing is owned and
    /// nobody waits.
    #[must_use]
    pub const fn new(base: [u32; T], ceilings: [u32; R], protocol: Protocol) -> Self {
        Self {
            base,
            ceilings,
            protocol,
            owners: [None; R],
            waiting: [None; T],
        }
    }

    /// Acquire or record one wait; the caller enforces the decision.
    ///
    /// # Errors
    ///
    /// [`Refusal::UnknownTask`], [`Refusal::UnknownResource`],
    /// [`Refusal::AboveCeiling`] or [`Refusal::Deadlock`]. Nothing changes.
    pub fn acquire(&mut self, task: usize, resource: usize) -> Result<Acquisition, Refusal> {
        let base = *self.base.get(task).ok_or(Refusal::UnknownTask)?;
        let ceiling = *self
            .ceilings
            .get(resource)
            .ok_or(Refusal::UnknownResource)?;
        if self.protocol == Protocol::ImmediateCeiling && base > ceiling {
            return Err(Refusal::AboveCeiling);
        }
        let owner = self.owner(resource);
        if owner == Some(task) {
            return Ok(Acquisition::AlreadyOwned);
        }
        let blocker = match owner {
            Some(_) => Some(resource),
            None => self.ceiling_blocker(task, base),
        };
        let Some(blocker) = blocker else {
            let (owner, wait) = self.slots_mut(task, resource)?;
            *owner = Some(task);
            *wait = None;
            return Ok(Acquisition::Granted);
        };
        let mut holder = self.owner(blocker);
        for _ in 0..T {
            let Some(owner) = holder else {
                break;
            };
            if owner == task {
                return Err(Refusal::Deadlock);
            }
            holder = self
                .waiting_on(owner)
                .and_then(|resource| self.owner(resource));
        }
        let (_, wait) = self.slots_mut(task, resource)?;
        *wait = Some(blocker);
        Ok(Acquisition::Blocked(blocker))
    }

    /// Release owned state and clear every wait on the resource, so the
    /// waiters retry explicitly. Ownership is never handed over.
    ///
    /// # Errors
    ///
    /// [`Refusal::UnknownTask`], [`Refusal::UnknownResource`],
    /// [`Refusal::Unowned`] when nobody owns the resource (a repeated release
    /// included) or [`Refusal::WrongOwner`]. Nothing changes.
    pub fn release(&mut self, task: usize, resource: usize) -> Result<(), Refusal> {
        if task >= T {
            return Err(Refusal::UnknownTask);
        }
        let owner = self
            .owners
            .get_mut(resource)
            .ok_or(Refusal::UnknownResource)?;
        match *owner {
            None => return Err(Refusal::Unowned),
            Some(current) if current != task => return Err(Refusal::WrongOwner),
            Some(_) => *owner = None,
        }
        for wait in &mut self.waiting {
            if *wait == Some(resource) {
                *wait = None;
            }
        }
        Ok(())
    }

    /// Cancel a wait without releasing owned resources.
    ///
    /// # Errors
    ///
    /// [`Refusal::UnknownTask`]. Nothing changes.
    pub fn cancel_wait(&mut self, task: usize) -> Result<Cancelling, Refusal> {
        let Some(wait) = self.waiting.get_mut(task) else {
            return Err(Refusal::UnknownTask);
        };
        Ok(match wait.take() {
            Some(_) => Cancelling::Cancelled,
            None => Cancelling::NotWaiting,
        })
    }

    /// Effective priorities for the runtime executor.
    #[must_use]
    pub fn effective_priorities(&self) -> [u32; T] {
        let mut effective = self.base;
        if self.protocol == Protocol::ImmediateCeiling {
            for (task, priority) in effective.iter_mut().enumerate() {
                *priority = self.ceiling_priority(task, *priority);
            }
        } else {
            for _ in 0..T {
                let mut changed = false;
                for (task, resource) in self.waiting.iter().enumerate() {
                    // `task` comes from enumerating `waiting`, which is as long
                    // as `effective`, so the lookup always lands; a missing
                    // owner is no edge to walk.
                    let Some(&inherited) = effective.get(task) else {
                        continue;
                    };
                    if let Some(slot) = resource
                        .and_then(|resource| self.owner(resource))
                        .and_then(|owner| effective.get_mut(owner))
                        && *slot < inherited
                    {
                        *slot = inherited;
                        changed = true;
                    }
                }
                if !changed {
                    break;
                }
            }
        }
        effective
    }

    /// The task that owns `resource`; `None` when it is free or no such
    /// resource exists.
    #[must_use]
    pub fn owner(&self, resource: usize) -> Option<usize> {
        self.owners.get(resource).copied().flatten()
    }

    /// The resource `task` has a wait recorded on; `None` when it is not
    /// waiting or no such task exists.
    #[must_use]
    pub fn waiting_on(&self, task: usize) -> Option<usize> {
        self.waiting.get(task).copied().flatten()
    }

    /// The protocol this arbiter was configured with.
    #[must_use]
    pub const fn protocol(&self) -> Protocol {
        self.protocol
    }

    /// Number of tasks (`T`).
    #[must_use]
    pub const fn task_count(&self) -> usize {
        T
    }

    /// Number of resources (`R`).
    #[must_use]
    pub const fn resource_count(&self) -> usize {
        R
    }

    /// The two slots `acquire` writes: `resource`'s owner and `task`'s wait.
    /// `acquire` has already looked both indices up, so the refusals only
    /// restate that; they are taken before anything is written.
    fn slots_mut(
        &mut self,
        task: usize,
        resource: usize,
    ) -> Result<(&mut Option<usize>, &mut Option<usize>), Refusal> {
        let wait = self.waiting.get_mut(task).ok_or(Refusal::UnknownTask)?;
        let owner = self
            .owners
            .get_mut(resource)
            .ok_or(Refusal::UnknownResource)?;
        Ok((owner, wait))
    }

    /// `base` raised to the ceiling of every resource `task` owns.
    fn ceiling_priority(&self, task: usize, base: u32) -> u32 {
        self.owners
            .iter()
            .zip(&self.ceilings)
            .filter(|(owner, _)| **owner == Some(task))
            .fold(base, |priority, (_, &ceiling)| priority.max(ceiling))
    }

    /// Under [`Protocol::ImmediateCeiling`], the resource another task owns
    /// whose ceiling stops `task` (base priority `base`) from taking a free
    /// resource now, if any.
    fn ceiling_blocker(&self, task: usize, base: u32) -> Option<usize> {
        if self.protocol == Protocol::Inheritance {
            return None;
        }
        let effective = self.ceiling_priority(task, base);
        self.owners
            .iter()
            .zip(&self.ceilings)
            .position(|(owner, &ceiling)| {
                owner.is_some_and(|owner| owner != task) && effective <= ceiling
            })
    }
}

#[cfg(test)]
mod tests;
