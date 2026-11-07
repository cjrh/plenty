# Timed waits and selection

Wait with the [typed deadline waits in `std.sync`](../../stdlib/sync.md#typed-deadline-waits)
in application code, and always where a stop request must end the wait. They
take `&Deadline` and `&CancellationToken` and recheck the token at least every
10 ms, so `cancel()` ends the wait with `WaitError.Cancelled`. The [signal lesson](../../../tutorial/11-concurrency/stop-on-signal.md#stop-a-long-wait-on-a-signal)
stops an hour-long selection on `SIGTERM` this way.

The millisecond primitives on this page are low-level, noncancellable waits.
Only `token.wait_timeout_ms` ends early, and only when its own token is
cancelled. No token wakes a channel, selection, or future wait, so a stop
request waits for the whole budget: a `select_recv_timeout_ms` call given an
hour delays shutdown by up to an hour.
Use them where nothing needs to interrupt the wait, or to build a wrapper that
checks its own token between short budgets, as `std.sync` does.

Each budget is `u64` milliseconds using a monotonic total budget, including repeated
wakeups. Zero checks immediately. Readiness or disconnection at the final check
wins over timeout. Lock contention, cleanup, and scheduling may delay return
beyond the budget; timeouts are not hard execution deadlines.
[`std.time`](../../stdlib/time.md#sleeping-and-timed-waits) converts a
`Duration` or the remainder of a shared `Deadline` to this budget.

| Operation | Result |
| --- | --- |
| `sender.send_timeout_ms(value, timeout_ms)` | `Result[(), SendTimeoutError[T]]` |
| `receiver.recv_timeout_ms(timeout_ms)` | `Result[T, RecvTimeoutError]` |
| `future.wait_timeout_ms(timeout_ms)` | `bool` |
| `token.wait_timeout_ms(timeout_ms)` | `bool` |

The original `_timeout` spellings remain compatibility aliases.

`SendTimeoutError[T]` has `Disconnected(value)` and `TimedOut(value)`, both
returning ownership of the unsent message without copying or allocating.
`RecvTimeoutError` has `Disconnected` and `TimedOut`; buffered messages remain
receivable after the last sender drops. All channel waits borrow endpoints.

A future wait observes completion or cancellation without consuming its result
or cancelling work. A token wait observes the permanent cancellation request
without consuming or resetting it. Neither timed observation allocates.

## Receive from either of two channels

| Operation | Result |
| --- | --- |
| `std.sync.select_recv_before(&first, &second, &deadline, &cancel)` | `Result[Selected[A, B], WaitError]`; cancellable |
| `select_recv(&first, &second)` | `Result[Selected[A, B], SelectError]` |
| `select_recv_nowait(&first, &second)` | Same |
| `select_recv_timeout_ms(&first, &second, timeout_ms)` | Same |

Selection borrows both receivers and transfers exactly one message.
`Selected[A, B]` has `First(A)` and `Second(B)` variants. Shared `&` markers are
optional, and two handles sharing one queue are accepted. The unselected queue
keeps its messages. Ordinary [message eligibility](channels.md) applies.

`SelectError.Empty` means neither had a message during a nonblocking check;
`TimedOut` means the budget expired. `Disconnected` means both queues are drained
and have no senders. One disconnected queue does not stop waiting for the other.
Blocking selection returns only Disconnected.

The first receiver has priority whenever both are ready; sustained traffic can
starve the second. Selection receives a message before returning, rather than
reporting readiness that another receiver could invalidate. It allocates nothing.

Only two-channel receive selection is supported: no send cases or arbitrary
channel sets. `select_recv_before` adds the deadline and cancellation token;
the builtins have neither. Selected and ordinary receives compete for
messages. Application protocols can still deadlock.
