# Timed waits and receive selection

Timeouts are `u64` milliseconds. They use a monotonic total budget, including
repeated wakeups. Zero checks immediately; no negative or floating-point timeout
is accepted. Readiness or disconnection observed at the final check takes
precedence over timeout. Mutex contention, cleanup, and scheduling can delay the
return; a timeout is not a hard execution deadline.

| Operation | Result |
| --- | --- |
| `sender.send_timeout(value, timeout_ms)` | `Result[(), SendTimeoutError[T]]` |
| `receiver.recv_timeout(timeout_ms)` | `Result[T, RecvTimeoutError]` |
| `future.wait_timeout(timeout_ms)` | `bool` |

`SendTimeoutError[T]` has `Disconnected(value)` and `TimedOut(value)` variants.
Both return ownership of the unsent value, including inline payloads, without
copying or allocating. `RecvTimeoutError` has `Disconnected` and `TimedOut`.
Buffered messages remain receivable after the last sender is dropped.

`future.wait_timeout` borrows the future. It returns true when its job has
completed or been cancelled, false if the budget expires first. It neither
cancels the job nor consumes the result. Call `result()` later to retrieve it.

## Select one of two receivers

| Operation | Result |
| --- | --- |
| `select_recv(&first, &second)` | `Result[Selected[A, B], SelectError]` |
| `select_recv_nowait(&first, &second)` | Same |
| `select_recv_timeout(&first, &second, timeout_ms)` | Same |

`Selected[A, B]` is an inline sum with `First(A)` and `Second(B)` variants.
Selection borrows both receivers and transfers exactly one message into this
result. The shared `&` markers are optional, as in ordinary shared arguments:
`select_recv(first, second)` also borrows both. The unselected queue keeps its
messages. It also accepts two handles
sharing one queue. Message types must satisfy the ordinary channel contract.

`SelectError.Empty` means neither receiver had a message during a nonblocking
check. `TimedOut` means the timed wait expired. `Disconnected` means **both**
queues are drained and have no senders; a single disconnected queue does not
prevent waiting for the other. Blocking selection returns only `Disconnected`.

The first receiver is checked before the second. When both have messages, the
first wins. There is no fairness guarantee: sustained traffic on the first can
starve the second. Put control messages first when that priority is intentional.

Selection receives the value while holding its queue lock. It does not return a
readiness hint that a competing receiver could invalidate. Stack-resident waiter
registrations and a private condition variable avoid polling, hidden allocation,
and a global selection lock. Registration and notification synchronize with the
queue; all registrations are removed before the call returns. User destructors
run outside synchronization locks.

The current API selects receives from two channels. It has no send cases,
arbitrary-size channel sets, or cancellation-token case. Ordinary and selected
receivers may compete for the same messages. As with ordinary channel waits,
application communication protocols can still deadlock.

[Crossbeam's selection API](https://docs.rs/crossbeam-channel/latest/crossbeam_channel/struct.Select.html)
distinguishes completed selection from readiness observation. Plenty receives
directly and uses explicit first-argument priority; it does not use randomized
selection or expose a separate operation that the caller must complete.

See the lessons on [bounded waits](../tutorial/92-bound-your-waits.md) and
[receiving from two channels](../tutorial/93-receive-from-either-channel.md).
