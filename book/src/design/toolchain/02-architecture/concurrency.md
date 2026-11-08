# Concurrent runtime mechanisms

Worker eligibility is a closed-world compiler check over finite type graphs,
reachable helpers, concrete callbacks, and destructors. Visited identities bound
recursive walks. Atomically counted owners can cross certified boundaries; counts
alone do not permit mutable sharing. The
[concurrency contracts](../../facilities/03-concurrency/index.md) define allowed
effects, owned jobs, blocking, and shutdown.

Each native primitive (`CollectionOp`) reports an effect from one exhaustive
match in `src/effects.rs`; the certification walk rejects a worker that can
reach a denied effect and names the call. A foreign call has no native
contract; it is rejected unless its declaration carries the binding author's
`@thread_safe` promise (`foreign::Declaration::thread_safe`). An opaque type
carries its declaration's contract (`foreign::Threads`) in `Ty::ForeignPtr`;
the walk tags each pending type with how it reaches the worker (held locally,
moved, an owning class field, or lent) and accepts a `Movable` pointer only
locally or as such a field. See
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
worker's inputs can reach. Each pool submission records the `Mutex[T]` types
its job input can reach, beside the sender message types, in the function's
submission list and in the future's local provenance; `result()` and each
shutdown run the same guard scan against them. Reachability walks through
`Shared[...]` contents.

## Shared owners

`Shared[T]` reuses the `Box[T]` runtime record and its type metadata kind: a
counted header, the content type, and a 32-byte-aligned content slot. The
frontend emits `BoxOp::Share` (the box allocation entry point) and
`BoxOp::SharedRef`, a shared-only projection to the content slot; it never
emits the box's move-out or mutable projection for a handle. `share()` is the
ordinary retain of a loaded owner. Construction rejects contents that reach an
executor or future, or that reach the same `Shared[T]` through a `Mutex` or a
channel endpoint.

Locking follows mutex 2 of Drepper's "Futexes Are Tricky" in the state word's
low two bits: 0 is unlocked, 1 locked, and 2 locked with possible waiters. Bit
2 (4) records `lock_when` waiters, which sleep on a separate sequence word.
Lock is an inline `atomic_cas` from 0 to 1. On failure,
`plenty_mutex_lock_slow` compare-exchanges the lock bits to 2, keeping bit 2,
and sleeps with `FUTEX_WAIT_PRIVATE` until it finds them 0; `EINTR` and
`EAGAIN` retry. Unlock is an inline `atomic_rmw` exchange to 0, and calls
`plenty_mutex_release` only when it replaced a value above 1: lock bits 2 wake
one lock waiter (`FUTEX_WAKE_PRIVATE`), and bit 2 increments the sequence word
and wakes all of its sleepers. `lock_now()` calls `plenty_mutex_try_lock` only
when its compare-exchange found 4, an unlocked word with condition waiters.
`plenty_mutex_wait`, one `lock_when` round, returns at once if the monotonic
deadline has passed; otherwise it reads the sequence word, exchanges the state
to 4 (waking a lock waiter if the lock bits were 2), sleeps on the sequence
word for at most 10 ms, and relocks with the slow path. Reading the sequence
under the lock means an unlock between the release and the sleep changes the
word first, so no wake is lost. The runtime issues the `futex` system call
directly; Apple targets use one shared pthread condition variable, and Windows
`WaitOnAddress`. Nothing allocates; a held lock is never abandoned by unwinding,
because traps terminate the process.
