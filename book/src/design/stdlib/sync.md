# `std.sync`

Exports `CancellationToken`, the existing
[cooperative cancellation facility](../facilities/03-concurrency/cancellation.md).
The imported and prelude types are identical; no wrapper, allocation, or new
effect permission is introduced by importing. Supported target: x86_64 Linux GNU.

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
Atomic integers and booleans are in [`std.atomic`](atomic.md). Mutexes,
semaphores and other synchronization primitives are not exported; this module
exposes only the adopted cancellation contract.
