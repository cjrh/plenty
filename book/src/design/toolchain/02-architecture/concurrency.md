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

## Atomics

`std.atomic` operations compile inline, with no runtime call. A method borrows
its receiver's place, so a local atomic lives in the function's frame and every
thread addresses the same slot. The payload is the slot's low word, naturally
aligned because slots are at least 8-byte aligned. Each access is a Cranelift
atomic instruction (`atomic_load`, `atomic_store`, `atomic_rmw`, `atomic_cas`,
`fence`) of the element's exact width, never a plain load the optimizer could
merge. Checked `fetch_add`/`fetch_sub` loop on `atomic_cas` and trap before
storing. Cranelift's atomics are all sequentially consistent, so each requested
`Ordering` is validated at compile time and then strengthened. On x86-64 a
load is a `mov`, a store and a fence add `mfence`, and read-modify-write
operations use `lock`-prefixed instructions or a `lock cmpxchg` loop.

## Mutexes

`std.sync.Mutex[T]` is the generic class in `stdlib/std/sync.plenty`: an
`Atomic[u32]` state word and the value, stored inline, so moves, destruction,
and worker eligibility are ordinary class behavior. Its fields are private;
`lock()` and `into_inner()` are compiler intrinsics. A lock block stores the
mutex reference in a hidden guard local and roots the value's exclusive loan
there, while the mutex's own shared loan stays live until the block's exit.
The ownership checker treats the guard as live from the lock to the unlock and
rejects a join reached while a live guard's `T` matches a `Mutex[T]` the
worker's inputs can reach.

Locking follows mutex 2 of Drepper's "Futexes Are Tricky": state 0 is
unlocked, 1 locked, and 2 locked with possible waiters. Lock is an inline
`atomic_cas` from 0 to 1. On failure, `plenty_mutex_lock_slow` exchanges in 2
and, until the exchange returns 0, sleeps with `FUTEX_WAIT_PRIVATE` on value 2;
`EINTR` and `EAGAIN` retry the exchange. Unlock is an inline `atomic_rmw`
exchange to 0, and calls `plenty_mutex_wake` (`FUTEX_WAKE_PRIVATE` of one
waiter) only when it replaced 2. The runtime issues the `futex` system call
directly. Nothing allocates; a held lock is never abandoned by unwinding,
because traps terminate the process.
