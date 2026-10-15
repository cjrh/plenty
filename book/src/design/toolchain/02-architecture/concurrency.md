# Concurrent runtime mechanisms

Worker eligibility is a closed-world compiler check over finite type graphs,
reachable helpers, concrete callbacks, and destructors. Visited identities bound
recursive walks. Atomically counted owners can cross certified boundaries; counts
alone do not permit mutable sharing. The
[concurrency contracts](../../facilities/03-concurrency/index.md) define allowed
effects, owned jobs, blocking, and shutdown.

Each native primitive (`CollectionOp`) reports an effect from one exhaustive
match in `src/effects.rs`; the certification walk rejects a worker that can
reach a denied effect and names the call. Primitives without a native contract,
such as foreign calls, are rejected separately. See
[native effects on worker threads](../../facilities/03-concurrency/effects.md).

## Tasks and queues

Scoped tasks pin argument/result storage in the parent's frame. Generated C
adapters invoke typed workers and publish results there. POSIX join must complete
before reclaiming storage; unexpected join failure terminates the process to
avoid a worker using departed storage. Task bookkeeping does not allocate a
Plenty owner; native thread stacks and OS bookkeeping still use resources.

Channels allocate a pinned core/ring whose slots reserve complete message layouts.
Endpoint values reference counted headers. One mutex protects predicates and
disconnect flags; separate condition variables serve readers and writers.
Disconnect wakes all waiters. Receive relocates values before slot reuse, and
user destructors run outside locks. `nowait` can still contend on a mutex.

Executors keep pinned worker records and a bounded ring. Partial startup failure
joins started workers. Each accepted job has one fallibly allocated cell for its
concrete environment and result; queue/worker and future owners use separate
atomic counts. Queue and job state have separate locks. User callbacks/destructors
run outside them; pool destruction drains and joins.

## Cancellation and selection

Tokens combine an atomic cancellation flag with a mutex/condition-variable
handshake for waiter registration. Timed waits track a monotonic elapsed budget,
so wakeups do not restart it and huge millisecond values need no overflowing
absolute deadline.

Receive selection registers pinned stack waiter nodes in participating queues
and uses a per-call condition variable. Queue-to-waiter lock ordering synchronizes
notification; selection transfers a value under its queue lock and removes all
registrations on every return. It needs no global selection lock or per-wait
heap allocation.
