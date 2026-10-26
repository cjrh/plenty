# `std.channel`

Explicit imports for [bounded channels](../facilities/03-concurrency/channels.md)
and [timed waits and selection](../facilities/03-concurrency/timed-selection.md).
Supported target: x86_64 Linux GNU. Imports are allocation-free aliases of the
existing prelude facilities, with identical generic type identity.

| Export | Contract |
|---|---|
| `channel[T](capacity: u64)` | `Result[tuple[Sender[T], Receiver[T]], ChannelError]` |
| `Sender[T]`, `Receiver[T]` | Owned endpoints; explicit `share()` retains the queue |
| `select_recv(&first, &second)` | `Result[Selected[A, B], SelectError]` |
| `select_recv_nowait(&first, &second)` | Same, without waiting for messages |
| `select_recv_timeout(&first, &second, timeout_ms: u64)` | Same, with a millisecond budget |
| `ChannelError` | `InvalidCapacity`, `Allocation(AllocError)` |
| `SendError[T]` | `Full(value)`, `Disconnected(value)` |
| `RecvError` | `Empty`, `Disconnected` |
| `SendTimeoutError[T]` | `TimedOut(value)`, `Disconnected(value)` |
| `RecvTimeoutError` | `TimedOut`, `Disconnected` |
| `Selected[A, B]` | `First(value)`, `Second(value)` |
| `SelectError` | `Empty`, `TimedOut`, `Disconnected` |

The linked facility pages specify every endpoint method. Capacity must be
positive and bounds queued messages. Construction makes one fallible allocation;
send, receive, share, error transport and selection allocate nothing. Messages
move without deep copying; rejected sends return the original value. Queue
operations have constant bookkeeping plus value transfer, with unbounded lock
and scheduling latency; destroying queued values costs their cleanup. Last
receiver drop destroys queued values and wakes blocked senders; last sender drop
allows buffered messages to drain before disconnection.

Messages must be wholly owned and worker-eligible. References, uncertified
foreign handles, and recursive receiver retention are still rejected. Imports
do not weaken the checks for live opposite endpoints across joins or future
waits. Timeouts are budgets, not hard deadlines. Use
[`std.time`](time.md) to compute remaining milliseconds; tokens do not interrupt
an ordinary channel wait. No unbounded channel or arbitrary selector is added.

## Acknowledgements

- Contracts follow Rust's [`sync_channel`](https://doc.rust-lang.org/std/sync/mpsc/fn.sync_channel.html) and [crossbeam-channel](https://docs.rs/crossbeam-channel/); no Crossbeam code is used.
