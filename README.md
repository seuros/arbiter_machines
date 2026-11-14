# ArbiterMachines

> When the slow job holds the key the urgent job needs, more workers won't help. 🛰️

A priority arbiter for Ruby. You declare your tasks with their priorities and your resources with their ceilings. The `Arbiter` tells you who may take what, who has to wait, who just inherited someone else's urgency, and who is about to deadlock. Your code still takes and releases the actual locks. The arbiter only ever answers one question: is this a good idea right now?

## Quick Start

```bash
gem 'arbiter_machines'
```

```ruby
# Two tasks: a janitor at priority 1, a payment at priority 10.
# One resource: the ledger lock, ceiling 10.
# Positional, like the Rust constructor: base priorities, ceilings, protocol.
arbiter = ArbiterMachines::Arbiter.new([1, 10], [10], :inheritance)

arbiter.acquire(0, 0)          # => :granted, janitor takes the ledger
arbiter.acquire(1, 0)          # => #<data ArbiterMachines::Blocked resource=0>, payment waits
arbiter.effective_priorities   # => [10, 10], janitor now runs as urgent

arbiter.release(0, 0)          # => :released, janitor done, wait cleared
arbiter.acquire(1, 0)          # => :granted, payment retries and gets in
```

Tasks and resources are indices into the arrays you passed in: `acquire(task, resource)`. Larger number means more urgent.

## A Message to the Resistance

So you gave the nightly cleanup job priority 1, the payment processor priority 10, and a pile of report builders priority 5. Sensible. Then one night the cleanup job took the ledger lock for a quick vacuum, the payment job arrived and blocked on it, and twelve report builders showed up and ran. All of them. Before the cleanup job got another slice of CPU.

The payment job, your most urgent work, sat behind priority-5 reports because a priority-1 job was holding its lock and nobody told the scheduler that the priority-1 job was suddenly very important.

That's priority inversion. It put a rover on Mars into a reboot loop in 1997, and it's been quietly doing the same thing to job queues ever since. The fix has been known just as long: while the janitor holds something the payment job needs, the janitor *is* the payment job. It inherits the urgency, finishes fast, hands the lock over.

This gem does the accounting for that. Who holds what, who waits on whom, what everybody's priority really is right now, and whether the next acquire closes a loop nobody can escape from.

## Chapter 1: Stardate 2025.186, the AMS Dijkstra

> **AMS Dijkstra**, NCC-4201, *Dijkstra*-class traffic-control vessel.
> One airlock. Two waiting crews. No circular holding pattern.

The vessel doesn't open airlocks. It keeps the chart: which crew is in which lock, which crew is waiting at which door, and which crew's urgency has been lent to the crew blocking it. Before any crew is cleared to wait, the chart is checked for a loop. A loop is never charted, because a charted loop is a ship nobody leaves.

Somewhere on the bridge, a junior officer suggests:

```ruby
def process_payment
  ledger_lock.synchronize do   # the janitor has this, and the janitor is slow
    charge_card
  end
end
```

"That," says the traffic controller, "is how urgent work waits for the floor to be mopped."

## The Flight Plan

Every resource is either free or held. Every task is either running, holding, or waiting at exactly one door.

```mermaid
stateDiagram-v2
    [*] --> free
    free --> held: acquire, granted
    held --> held: holder acquires again, already_owned (nothing changes)
    held --> held: other task acquires, Blocked (holder inherits urgency)
    held --> free: holder releases, waiters cleared to retry
    held --> held: acquire would close a wait loop, deadlock (nothing changes)
    free --> [*]
```

And the night the janitor held the ledger:

```mermaid
sequenceDiagram
    participant J as Janitor (task 0, prio 1)
    participant A as Arbiter
    participant P as Payment (task 1, prio 10)

    J->>A: acquire(0, 0)
    A-->>J: granted
    Note over J: starts vacuuming the ledger

    P->>A: acquire(1, 0)
    A-->>P: Blocked(resource 0)
    Note over A: effective priorities now [10, 10]
    Note over J: runs as priority 10, no report builder gets ahead

    J->>A: release(0, 0)
    A-->>J: released
    Note over A: effective priorities back to [1, 10], wait cleared

    P->>A: acquire(1, 0)
    A-->>P: granted
```

## How Arbitration Works

### Acquire

`acquire(task, resource)` answers `:granted`, `:already_owned`, or a `Blocked` naming the resource you're waiting on. Anything else is a refusal symbol.

```ruby
arbiter = ArbiterMachines::Arbiter.new([1, 5, 10], [10, 10], :inheritance)

arbiter.acquire(0, 0)   # => :granted
arbiter.acquire(0, 0)   # => :already_owned, a repeat: nothing changes
arbiter.acquire(1, 1)   # => :granted
arbiter.acquire(1, 0)   # => #<data ArbiterMachines::Blocked resource=0>
arbiter.acquire(1, 0)   # => the same Blocked again, nothing changes
arbiter.owner(0)        # => 0
arbiter.waiting_on(1)   # => 0
```

Task 1 now holds resource 1 and is waiting on resource 0, which task 0 holds.

### Inheritance is transitive

Urgency flows up the chain. If a third task waits on task 1, task 1 gets its priority, and so does task 0 behind it.

```ruby
arbiter.acquire(2, 1)          # => #<data ArbiterMachines::Blocked resource=1>
arbiter.effective_priorities   # => [10, 10, 10]
```

Task 2, priority 10, waits on task 1. Task 1 waits on task 0. Everybody in the chain runs at 10 until the chain clears.

### Release and retry

Releasing clears the waits on that resource. It does not hand the resource to anyone. The waiters call `acquire` again, and the arbiter decides fresh.

```ruby
arbiter.release(0, 0)          # => :released
arbiter.effective_priorities   # => [1, 10, 10]  task 1 still blocks task 2
arbiter.acquire(1, 0)          # => :granted
```

Release by a task that doesn't hold the resource is `:wrong_owner`, and of a resource nobody holds `:unowned`. Both change nothing. A second release of the same resource is `:unowned` too: the arbiter keeps no history, so it can't prove you were the one who held it.

### Stop waiting

A task that gives up on a door calls `cancel_wait`. Its held resources stay held. Only the wait is dropped, and the urgency it was lending goes with it.

```ruby
arbiter = ArbiterMachines::Arbiter.new([1, 5, 10], [10, 10], :inheritance)
arbiter.acquire(0, 0)
arbiter.acquire(2, 0)          # => #<data ArbiterMachines::Blocked resource=0>
arbiter.effective_priorities   # => [10, 5, 10]

arbiter.cancel_wait(2)         # => :cancelled
arbiter.cancel_wait(2)         # => :not_waiting, a repeat: nothing changes
arbiter.effective_priorities   # => [1, 5, 10]
```

### Deadlock is refused, not reported after the fact

Two tasks each holding what the other wants is a loop. The arbiter sees it before charting the second wait and returns `:deadlock`. Nothing about the current state changes, so you can back out cleanly.

```ruby
arbiter = ArbiterMachines::Arbiter.new([1, 5, 10], [10, 10], :inheritance)
arbiter.acquire(0, 0)
arbiter.acquire(1, 1)
arbiter.acquire(0, 1)          # => #<data ArbiterMachines::Blocked resource=1>
arbiter.acquire(1, 0)          # => :deadlock
arbiter.effective_priorities   # => [1, 5, 10]  untouched

arbiter.cancel_wait(0)         # => :cancelled, task 0 backs off
arbiter.acquire(1, 0)          # => #<data ArbiterMachines::Blocked resource=0>  now a plain wait
```

### The other protocol: immediate ceiling

With `:immediate_ceiling`, a task jumps to the resource's ceiling the moment it takes the resource, before anyone has to wait. Nobody with a priority at or below the ceiling can get in behind it.

```ruby
arbiter = ArbiterMachines::Arbiter.new([1, 5, 10], [8, 10], :immediate_ceiling)
arbiter.acquire(0, 0)          # => :granted
arbiter.effective_priorities   # => [8, 5, 10]   task 0 raised to the ceiling
arbiter.acquire(1, 1)          # => Blocked resource=0, priority 5 can't clear a ceiling of 8
arbiter.acquire(2, 1)          # => :granted, priority 10 can
```

A ceiling has to be at least as high as every task that will ever take the resource. Task 2 at priority 10 asking for resource 0 with ceiling 8 is a misdeclaration, and the arbiter says so:

```ruby
arbiter.acquire(2, 0)   # => :above_ceiling
```

## The Rules of the Vessel

- **Ruby mirrors the Rust core.** `Arbiter.new(base, ceilings, protocol)` takes its arguments positionally, in the order of `Arbiter::new`. `acquire(task, resource)`, `release(task, resource)`, `cancel_wait(task)`, `effective_priorities`, `owner(resource)`, `waiting_on(task)`, `protocol`, `task_count` and `resource_count` are the core's methods under the core's names.
- **Up to 8 tasks and 8 resources**, chosen independently by the lengths of `base` and `ceilings`: every count in `0..8` is compiled in, and `Arbiter::TASK_CAPACITY` and `Arbiter::RESOURCE_CAPACITY` say so. A longer array raises `RangeError`.
- **Priorities and ceilings are Integers** in `0..2**32-1`; **indices are non-negative Integers**. Floats, strings and nil raise `TypeError`, so does a `base` or `ceilings` that isn't an Array or a `protocol` that isn't a Symbol. A value out of range, or a protocol other than `:inheritance` and `:immediate_ceiling`, raises `RangeError`. `ArgumentError` is only ever Ruby's own, for the wrong number of arguments.
- **An index past your tasks or resources is a refusal, not an exception**: `:unknown_task` or `:unknown_resource`. `owner` and `waiting_on` answer `nil` for it.
- **Refusals don't raise and don't change anything.** Not an owner, not a wait, not a priority. Every reason has its own symbol, the snake case of the core's `Refusal` variant; there is no catch-all.
- **A task waits or runs, never both.** A task that calls `acquire` is running, so `:granted` drops a wait it had on another resource and a new `Blocked` replaces it. The answer doesn't name the dropped wait: you already have it (the `Blocked` you got and didn't cancel), and `waiting_on(task)` reads it before the call. If you keep your own waiter queues, call `cancel_wait` when a task gives up a wait.
- **Repeats are honoured when the arbiter can prove them.** `acquire` of a resource the task already owns is `:already_owned`. `acquire` while the same resource still blocks the task answers the same `Blocked` again. `cancel_wait` of a task that isn't waiting is `:not_waiting`. None of them changes anything. A repeated `release` can't be proven, because the arbiter keeps no history of who held what: it is `:unowned`.
- **`Blocked` is a value, and values are valid or don't exist.** `ArbiterMachines::Blocked.new(resource)` is checked by the native extension whether the arbiter built it or you did: a non-Integer raises `TypeError`, a negative or oversized index `RangeError`. `==` compares it by value. It is sealed: subclassing it raises `TypeError` and `allocate` is gone. Sealing stops accidents, not reflection (a named `class X < Blocked` still binds `X` before raising, and `Data#initialize` can be bound directly), but the arbiter reads no `Blocked` from you, so a forged one reaches nothing and never equals a real answer.
- **No scopes, no restarts to plan for.** The arbiter issues no ids, tokens or handles; tasks and resources are your own indices. A new arbiter starts with nothing owned and nobody waiting.
- **No clock.** The arbiter reads no time and records none. Nothing it answers depends on when you ask.
- **Every answer, by method.**

  | Method | Honoured | Refused | Records the clock |
  | --- | --- | --- | --- |
  | `acquire(task, resource)` | `:granted`, `:already_owned` (a repeat), a `Blocked` (the wait is recorded; the same `Blocked` again changes nothing) | `:unknown_task`, `:unknown_resource`, `:above_ceiling`, `:deadlock` | no clock, nothing recorded |
  | `release(task, resource)` | `:released` | `:unknown_task`, `:unknown_resource`, `:unowned` (a repeated release included), `:wrong_owner` | no clock, nothing recorded |
  | `cancel_wait(task)` | `:cancelled`, `:not_waiting` (a repeat) | `:unknown_task` | no clock, nothing recorded |
  | `effective_priorities` | an Array of Integers, one per task, fresh every call | never refuses | no clock, nothing recorded |
  | `owner(resource)` | the owning task, `nil` when free or no such resource | never refuses | no clock, nothing recorded |
  | `waiting_on(task)` | the resource waited on, `nil` when not waiting or no such task | never refuses | no clock, nothing recorded |
  | `Blocked.new(resource)` | the value | never refuses; an invalid value raises `RangeError` (`TypeError` for a non-Integer) | no clock, nothing recorded |

- **Frozen means frozen.** `freeze` the arbiter and `acquire`, `release`, and `cancel_wait` raise `FrozenError`. Reads still work.
- **The binding never loses work.** A `Blocked` answer is built by calling `Blocked.new`, which is Ruby code, so `acquire` decides on a copy and keeps it only once the answer exists. If building it raises, that exception propagates and nothing is applied. If it freezes the arbiter, `FrozenError`; if it calls back in and changes the arbiter, `RuntimeError`; in both cases nothing is applied. The `Blocked` class is resolved once, when the extension loads: reassigning the constant later changes nothing the arbiter builds.
- **Ceilings are always required**, even under `:inheritance`, where they're not consulted.

## What You Think You're Doing vs Reality

### You Think: "Priority 10 means it runs first."
### Reality: Priority 10 means it waits first, behind whoever holds its lock.

A priority is a promise about scheduling, not about locks. The moment a high-priority task blocks on a lock held by a low-priority one, every medium task in the system is effectively more urgent than your most urgent work. Inheritance is what makes the number mean something again.

## What's Yours

The arbiter never touches a mutex, a thread, or a scheduler. When it says `:granted`, you take the lock. When it answers a `Blocked`, you wait however you wait, then ask again after the holder releases. When it reports an effective priority, you're the one who has to tell your scheduler. Keep one arbiter per group of resources, and keep its calls serialized.

## License

MIT License. See [LICENSE.txt](LICENSE.txt) for details.

**Remember: Nobody can hear the payment job scream while the janitor mops at priority 1.**
