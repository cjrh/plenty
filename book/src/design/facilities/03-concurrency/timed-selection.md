# Timed waits and selection

Timeouts are `u64` milliseconds using a monotonic total budget, including repeated
wakeups. Zero checks immediately. Readiness or disconnection at the final check
wins over timeout. Lock contention, cleanup, and scheduling may delay return
beyond the budget; timeouts are not hard execution deadlines.
[`std.time`](../../stdlib/time.md#sleeping-and-timed-waits) converts a
`Duration` or the remainder of a shared `Deadline` to this budget.

| Operation | Result |
| --- | --- |
| `sender.send_timeout(value, timeout_ms)` | `Result[(), SendTimeoutError[T]]` |
| `receiver.recv_timeout(timeout_ms)` | `Result[T, RecvTimeoutError]` |
| `future.wait_timeout(timeout_ms)` | `bool` |
| `token.wait_timeout(timeout_ms)` | `bool` |

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
| `select_recv(&first, &second)` | `Result[Selected[A, B], SelectError]` |
| `select_recv_nowait(&first, &second)` | Same |
| `select_recv_timeout(&first, &second, timeout_ms)` | Same |

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

Only two-channel receive selection is supported: no send cases, arbitrary channel
sets, or cancellation-token case. Selected and ordinary receives compete for
messages. Application protocols can still deadlock.
