#![no_std]
#![doc = include_str!("../README.md")]

/// Resource priority protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Protocol {
    /// Transitive inheritance from blocked waiters.
    Inheritance,
    /// Immediate ceilings plus system-ceiling admission.
    ImmediateCeiling,
}
/// Acquisition decision; no underlying lock is manipulated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Acquisition {
    /// Caller may acquire the resource.
    Granted,
    /// Caller owns it; recursion depth is not tracked.
    AlreadyOwned,
    /// Caller must wait on this blocking resource index.
    Blocked(usize),
    /// This wait would create a cycle; old state is retained.
    Deadlock,
    /// Invalid index or declared ceiling.
    Invalid,
}
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
    /// Configure base priorities and resource ceilings.
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
    #[must_use]
    pub fn acquire(&mut self, task: usize, resource: usize) -> Acquisition {
        if task >= T || resource >= R {
            return Acquisition::Invalid;
        }
        if self.protocol == Protocol::ImmediateCeiling && self.base[task] > self.ceilings[resource]
        {
            return Acquisition::Invalid;
        }
        if self.owners[resource] == Some(task) {
            return Acquisition::AlreadyOwned;
        }
        let blocker = if self.owners[resource].is_some() {
            Some(resource)
        } else if self.protocol == Protocol::ImmediateCeiling {
            let effective = self.effective_priorities()[task];
            self.owners
                .iter()
                .enumerate()
                .find(|&(index, owner)| {
                    owner.is_some_and(|owner| owner != task) && effective <= self.ceilings[index]
                })
                .map(|(index, _)| index)
        } else {
            None
        };
        let Some(blocker) = blocker else {
            self.owners[resource] = Some(task);
            self.waiting[task] = None;
            return Acquisition::Granted;
        };
        let mut holder = self.owners[blocker];
        for _ in 0..T {
            let Some(owner) = holder else {
                break;
            };
            if owner == task {
                return Acquisition::Deadlock;
            }
            holder = self.waiting[owner].and_then(|resource| self.owners[resource]);
        }
        self.waiting[task] = Some(blocker);
        Acquisition::Blocked(blocker)
    }
    /// Release owned state and clear affected waits for explicit retries.
    #[must_use]
    pub fn release(&mut self, task: usize, resource: usize) -> bool {
        let Some(owner) = self.owners.get_mut(resource) else {
            return false;
        };
        if *owner != Some(task) {
            return false;
        }
        *owner = None;
        for wait in &mut self.waiting {
            if *wait == Some(resource) {
                *wait = None;
            }
        }
        true
    }
    /// Cancel a wait without releasing owned resources.
    #[must_use]
    pub fn cancel_wait(&mut self, task: usize) -> bool {
        let Some(wait) = self.waiting.get_mut(task) else {
            return false;
        };
        *wait = None;
        true
    }
    /// Effective priorities for the runtime executor.
    #[must_use]
    pub fn effective_priorities(&self) -> [u32; T] {
        let mut effective = self.base;
        if self.protocol == Protocol::ImmediateCeiling {
            for (resource, owner) in self.owners.iter().enumerate() {
                if let Some(owner) = owner {
                    effective[*owner] = effective[*owner].max(self.ceilings[resource]);
                }
            }
        } else {
            for _ in 0..T {
                let mut changed = false;
                for (task, resource) in self.waiting.iter().enumerate() {
                    if let Some(owner) = resource.and_then(|resource| self.owners[resource])
                        && effective[owner] < effective[task]
                    {
                        effective[owner] = effective[task];
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
}
#[cfg(test)]
mod tests;
