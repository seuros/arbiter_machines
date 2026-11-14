# arbiter-machines

> **AMS Dijkstra**, NCC-4201, *Dijkstra*-class traffic-control vessel.

Engine-room flight manual: coordinate airlock ownership without a circular wait.

Allocation-free `no_std` ownership and wait-for decisions for fixed tasks/resources.
Larger numeric priority means higher priority. One outstanding wait per task;
nonrecursive ownership. Releases wake affected waiters to retry but never silently
transfer ownership.

Inheritance propagates priorities transitively to owners. Immediate-ceiling mode
raises owners to held-resource ceilings and blocks acquisition unless effective
priority exceeds every ceiling held by another task. Each configured resource
ceiling must bound all tasks permitted to use it; requests exceeding it are refused.
The core detects prospective wait-for cycles before inserting a wait.

These decisions require an executor honoring priorities and ownership. No OS
priority manipulation, lock implementation, waiter-queue fairness, preemption
driver, multi-resource atomic acquisition, or Ruby scheduling guarantee.

## Contract

- **Refusals.** `acquire`, `release` and `cancel_wait` return
  `Result<_, Refusal>`; `Refusal` is the crate's one refusal type:
  `UnknownTask`, `UnknownResource`, `AboveCeiling` (immediate ceiling only: the
  task's base priority exceeds the resource's declared ceiling), `Deadlock`
  (recording the wait would close a wait-for cycle), `Unowned` (nobody owns the
  resource), `WrongOwner`. A refusal changes nothing: no owner, no wait, no
  priority moves.
- **Honoured answers.** `Acquisition::{Granted, AlreadyOwned, Blocked(r)}`,
  `release -> Ok(())`, `Cancelling::{Cancelled, NotWaiting}`. `Blocked(r)` is a
  state change, not a refusal: the task's wait on `r` is now recorded and its
  priority is lent to the owner.
- **Waiting or running.** A task either waits or runs; a task that calls
  `acquire` is running, so it has stopped waiting. `Granted` therefore clears
  a wait the task had recorded on another resource, and `Blocked(r)` replaces
  it. The answer does not name the dropped wait: the caller already holds it
  (the `Blocked` it was given and has not cancelled) and `waiting_on(task)`
  reads it before the call. A caller keeping its own waiter queues calls
  `cancel_wait` when a task gives up a wait, which keeps both views equal.
- **Repeats.** Honoured when the arbiter can prove the effect already holds,
  and then nothing changes: `AlreadyOwned` (the task owns the resource),
  `Blocked(r)` again while `r` still blocks the task (that wait is already
  recorded), `NotWaiting` (no wait recorded). A repeated `release` cannot be
  proven, because no ownership history is kept: it is `Unowned`.
- **Exhaustive.** `Protocol`, `Acquisition`, `Cancelling` and `Refusal` carry
  no `#[non_exhaustive]`; a new variant is a breaking change.
- **Scopes and restarts.** The arbiter issues no ids: tasks and resources are
  caller indices below `T` and `R`. A new arbiter owns nothing and has no waits.
- **Time.** None. The arbiter reads and records no clock.
- **Readers.** `effective_priorities`, `owner`, `waiting_on`, `protocol`,
  `task_count` (`T`) and `resource_count` (`R`) are pure.
