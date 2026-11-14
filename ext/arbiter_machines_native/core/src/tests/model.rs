//! Randomized comparison against a plain reference model.
use super::*;
use std::vec::Vec;

const TASKS: usize = 4;
const RESOURCES: usize = 3;

struct Model {
    base: Vec<u32>,
    ceilings: Vec<u32>,
    protocol: Protocol,
    owners: Vec<Option<usize>>,
    waiting: Vec<Option<usize>>,
}

impl Model {
    /// Priority by definition: the base, raised by owned ceilings or by
    /// everyone waiting on something this task owns, transitively.
    fn effective(&self, task: usize) -> u32 {
        let mut priority = self.base[task];
        for (resource, owner) in self.owners.iter().enumerate() {
            if *owner != Some(task) {
                continue;
            }
            if self.protocol == Protocol::ImmediateCeiling {
                priority = priority.max(self.ceilings[resource]);
                continue;
            }
            for (waiter, wait) in self.waiting.iter().enumerate() {
                if *wait == Some(resource) {
                    priority = priority.max(self.effective(waiter));
                }
            }
        }
        priority
    }

    /// Whether `from` already waits, directly or not, on something `task` owns.
    fn reaches(&self, from: usize, task: usize) -> bool {
        let mut seen = Vec::new();
        let mut at = Some(from);
        while let Some(current) = at {
            if current == task {
                return true;
            }
            if seen.contains(&current) {
                return false;
            }
            seen.push(current);
            at = self.waiting[current].and_then(|resource| self.owners[resource]);
        }
        false
    }

    fn acquire(&mut self, task: usize, resource: usize) -> Result<Acquisition, Refusal> {
        if task >= TASKS {
            return Err(Refusal::UnknownTask);
        }
        if resource >= RESOURCES {
            return Err(Refusal::UnknownResource);
        }
        let ceiling = self.protocol == Protocol::ImmediateCeiling;
        if ceiling && self.base[task] > self.ceilings[resource] {
            return Err(Refusal::AboveCeiling);
        }
        if self.owners[resource] == Some(task) {
            return Ok(Acquisition::AlreadyOwned);
        }
        let mut blocker = self.owners[resource].map(|_| resource);
        if blocker.is_none() && ceiling {
            let mine = self.effective(task);
            blocker = (0..RESOURCES).find(|&index| {
                self.owners[index].is_some_and(|owner| owner != task)
                    && mine <= self.ceilings[index]
            });
        }
        let Some(blocker) = blocker else {
            self.owners[resource] = Some(task);
            self.waiting[task] = None;
            return Ok(Acquisition::Granted);
        };
        if self.owners[blocker].is_some_and(|owner| self.reaches(owner, task)) {
            return Err(Refusal::Deadlock);
        }
        self.waiting[task] = Some(blocker);
        Ok(Acquisition::Blocked(blocker))
    }

    fn release(&mut self, task: usize, resource: usize) -> Result<(), Refusal> {
        if task >= TASKS {
            return Err(Refusal::UnknownTask);
        }
        if resource >= RESOURCES {
            return Err(Refusal::UnknownResource);
        }
        match self.owners[resource] {
            None => Err(Refusal::Unowned),
            Some(owner) if owner != task => Err(Refusal::WrongOwner),
            Some(_) => {
                self.owners[resource] = None;
                for wait in &mut self.waiting {
                    if *wait == Some(resource) {
                        *wait = None;
                    }
                }
                Ok(())
            }
        }
    }

    fn cancel_wait(&mut self, task: usize) -> Result<Cancelling, Refusal> {
        if task >= TASKS {
            return Err(Refusal::UnknownTask);
        }
        Ok(match self.waiting[task].take() {
            Some(_) => Cancelling::Cancelled,
            None => Cancelling::NotWaiting,
        })
    }
}

struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % bound as u64).expect("below a usize bound")
    }

    fn priority(&mut self, bound: u64) -> u32 {
        u32::try_from(self.next() % bound).expect("below a u32 bound")
    }
}

fn agrees(arbiter: &Arbiter<TASKS, RESOURCES>, model: &Model) {
    for resource in 0..RESOURCES {
        assert_eq!(arbiter.owner(resource), model.owners[resource]);
    }
    for task in 0..TASKS {
        assert_eq!(arbiter.waiting_on(task), model.waiting[task]);
        assert_eq!(arbiter.effective_priorities()[task], model.effective(task));
    }
}

fn run(seed: u64, protocol: Protocol) -> [usize; 3] {
    let mut random = Random(seed);
    let base: [u32; TASKS] = core::array::from_fn(|_| random.priority(6));
    let ceilings: [u32; RESOURCES] = core::array::from_fn(|_| 2 + random.priority(5));
    let mut arbiter = Arbiter::new(base, ceilings, protocol);
    let mut model = Model {
        base: base.to_vec(),
        ceilings: ceilings.to_vec(),
        protocol,
        owners: [None; RESOURCES].to_vec(),
        waiting: [None; TASKS].to_vec(),
    };
    // Honoured changes, honoured repeats, refusals.
    let mut tally = [0; 3];
    for _ in 0..400 {
        let before = arbiter.clone();
        let task = random.below(TASKS + 1);
        let resource = random.below(RESOURCES + 1);
        let unchanged = match random.below(5) {
            0 | 1 => {
                let answer = arbiter.acquire(task, resource);
                assert_eq!(answer, model.acquire(task, resource));
                match answer {
                    Ok(Acquisition::Granted) => Some(false),
                    Ok(Acquisition::Blocked(on)) => Some(before.waiting_on(task) == Some(on)),
                    Ok(Acquisition::AlreadyOwned) => Some(true),
                    Err(_) => None,
                }
            }
            2 | 3 => {
                let answer = arbiter.release(task, resource);
                assert_eq!(answer, model.release(task, resource));
                answer.is_ok().then_some(false)
            }
            _ => {
                let answer = arbiter.cancel_wait(task);
                assert_eq!(answer, model.cancel_wait(task));
                match answer {
                    Ok(Cancelling::Cancelled) => Some(false),
                    Ok(Cancelling::NotWaiting) => Some(true),
                    Err(_) => None,
                }
            }
        };
        match unchanged {
            Some(false) => tally[0] += 1,
            Some(true) => {
                tally[1] += 1;
                assert_eq!(arbiter, before, "an honoured repeat changed the arbiter");
            }
            None => {
                tally[2] += 1;
                assert_eq!(arbiter, before, "a refusal changed the arbiter");
            }
        }
        agrees(&arbiter, &model);
    }
    tally
}

#[test]
fn random_operations_match_the_reference_model() {
    let mut seen = [0; 3];
    for seed in 1..=200_u64 {
        for protocol in [Protocol::Inheritance, Protocol::ImmediateCeiling] {
            let counts = run(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15), protocol);
            for (total, count) in seen.iter_mut().zip(counts) {
                *total += count;
            }
        }
    }
    assert!(seen.iter().all(|&count| count > 1000), "{seen:?}");
}
