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
ceiling must bound all tasks permitted to use it; requests exceeding it reject.
The core detects prospective wait-for cycles before inserting a wait.

These decisions require an executor honoring priorities and ownership. No OS
priority manipulation, lock implementation, waiter-queue fairness, preemption
driver, multi-resource atomic acquisition, or Ruby scheduling guarantee.
