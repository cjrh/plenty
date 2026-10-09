# Cooperative cancellation

`CancellationToken()` returns `Result[CancellationToken, AllocError]`. The token
is an affine handle to one shared cancellation state. Move it normally or call
`share()` to create another handle without allocating. Tokens may cross threads
and live in ordinary owned containers.

| Operation | Result | Meaning |
| --- | --- | --- |
| `token.share()` | `CancellationToken` | Share the same cancellation state |
| `token.cancel()` | `()` | Request cancellation permanently; wake all token waiters |
| `token.is_cancelled()` | `bool` | Observe whether cancellation was requested |
| `token.wait()` | `()` | Wait until cancellation is requested |
| `token.wait_timeout(timeout_ms)` | `bool` | Wait up to a `u64` millisecond budget; true means cancelled |

These methods borrow the receiver and do not require a mutable binding. `cancel`
is idempotent. Dropping a handle does not request cancellation; the last handle
releases the shared state. Copying and equality are not supported.

Cancellation is a request, not a forced interruption. Workers check the token at
safe boundaries, finish any required cleanup, and return through ordinary control
flow. The token does not choose a worker's return type or error value. Existing
`Future.cancel()` still only prevents a pending job from starting; requesting a
token does not change the future's state by itself.

An ordinary blocking channel receive or foreign call does not observe a token.
Use [timed waits](39-timed-waits-and-selection.md) to regain control periodically,
or design an explicit shutdown message. Scope exit and executor shutdown still
join running workers, so cancellation-aware work must actually return.

Construction performs one fallible allocation. Sharing, requesting, observing,
waiting, and dropping allocate nothing. Polling reads an atomic flag; a mutex
synchronizes cancellation with waiter registration, and a condition variable
wakes all waiters. Timed waits use a monotonic total budget
and do not restart it after wakeups. A zero budget checks immediately. A timeout
does not reset or consume the token. Lock contention and scheduling can delay
return beyond the budget; this is not a real-time deadline.

See the [runnable lesson](../tutorial/91-stop-work-cooperatively.md).
