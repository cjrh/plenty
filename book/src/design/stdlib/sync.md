# `std.sync`

Exports `CancellationToken`, the existing
[cooperative cancellation facility](../facilities/03-concurrency/cancellation.md),
and `Mutex[T]`. The imported and prelude `CancellationToken` types are
identical; no wrapper, allocation, or new effect permission is introduced by
importing. `Mutex` is not a prelude name: each use needs an import, through any
item or module alias. Supported target: x86_64 Linux GNU.

`CancellationToken() -> Result[CancellationToken, AllocError]` creates an owned
shared state in one fallible allocation.

| Method | Result | Effect |
|---|---|---|
| `share()` | `CancellationToken` | Explicitly retain the same state |
| `cancel()` | `()` | Permanently request cancellation and wake token waiters |
| `is_cancelled()` | `bool` | Observe the request |
| `wait()` | `()` | Wait until cancellation is requested |
| `wait_timeout(timeout_ms: u64)` | `bool` | Observe cancellation within the budget |

Methods borrow the handle, allocate nothing, and copy no captured data. Share
and observation have constant bookkeeping; cancellation also wakes waiters.
Lock contention and scheduling have no bounded latency. Tokens are safe to move
or explicitly share into eligible workers; borrowed handles retain ordinary
loans. Dropping the last handle releases the state but does not request stopping.
Cancellation is idempotent and does not reset.

A token does not interrupt instructions, cancel a future, or wake a channel
wait. Workers must return cooperatively, using bounded waits where needed.
[`std.time`](time.md) supplies deadline arithmetic and timeout conversion.

## `Mutex[T]`

A value that workers update through shared references or
[`Shared[Mutex[T]]`](../facilities/01-builtins/shared.md) handles, one lock
holder at a time. See [mutexes](../facilities/03-concurrency/mutex.md) for the rules
and the deadlocks the compiler rejects.

```plenty
from std.sync import Mutex

class Stats:
    count: i64
    total: i64

def record(stats: &Mutex[Stats], value: i64) -> ():
    with stats.lock() as data:
        data.count = data.count + 1
        data.total = data.total + value

def main() -> Result[(), ThreadError | IoError]:
    stats = Mutex[Stats](Stats(0, 0))
    with spawn(record, &stats, 3)?, spawn(record, &stats, 4)?:
        pass
    final = stats.into_inner()
    print(final.total)?
    Ok(())
```

| Operation | Result | Effect |
|---|---|---|
| `Mutex[T](value)`, `Mutex(value)` | `Mutex[T]` | Store `value` unlocked, inline |
| `with mutex.lock() as value:` | binds `value: &mut T` | Wait for the lock, hold it for the block |
| `mutex.into_inner()` | `T` | Consume an owned mutex and return its value |

`T` is any type a generic class field can hold: no references, generators, or
borrowed environments. A mutex is affine, with no copy, equality, or
formatting. It is stored inline (a 32-bit state word beside `T`); construction,
locking, and unlocking allocate nothing. An uncontended lock and unlock are
inline atomic instructions; a contended one waits in the kernel.

`lock()` is valid only as a `with` manager. It borrows the mutex shared, so no
`mut` is needed, and every exit from the block unlocks. There is no
`try_lock`, timed lock, poisoning, reentrant locking, or fairness guarantee.
`into_inner()` needs no lock: owning the mutex proves no other thread can hold
it.

Atomic integers and booleans are in [`std.atomic`](atomic.md). The shared
owner `Shared[T]` is a [prelude type](../facilities/01-builtins/shared.md).
Semaphores, condition variables, and read-write locks are not exported.
