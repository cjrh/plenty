# Bounded channels

`channel[T](capacity)` creates a bounded, multi-producer, multi-consumer FIFO queue.
Its `u64` capacity must be positive. The result is
`Result[tuple[Sender[T], Receiver[T]], ChannelError]`.
Zero returns `ChannelError.InvalidCapacity`; allocation failure or size overflow
returns `ChannelError.Allocation(AllocError)`. Construction is transactional.

## Operations and ownership

| Operation | Result | Meaning |
| --- | --- | --- |
| `sender.send(value)` | `Result[(), SendError[T]]` | Wait for space or receiver disconnection |
| `sender.send_nowait(value)` | `Result[(), SendError[T]]` | Return `Full(value)` immediately if no space exists |
| `receiver.recv()` | `Result[T, RecvError]` | Wait for a message or sender disconnection |
| `receiver.recv_nowait()` | `Result[T, RecvError]` | Return `Empty` immediately if no message is ready |
| `endpoint.share()` | The same endpoint type | Create another handle to the same queue without allocating |

Sending follows ordinary value-transfer rules: mutable owners move; copyable
values retain their usual assignment semantics. Both send operations return
`SendError[T].Disconnected(value)` if no receiver remains; the error owns the
unsent value. `send_nowait` also returns `SendError[T].Full(value)`. Blocking send
never returns `Full`. Failure does not discard or copy the message.

Receive transfers ownership of one message to one receiver. Shared receivers
compete for messages; they do not subscribe to a broadcast. Accepted messages are
received in queue insertion order. Concurrent senders have no predetermined
ordering, and waiting threads have no strict fairness guarantee.

Methods borrow the endpoint through its ordinary shared receiver. Bindings do not
need `mut`; the queue is explicitly synchronized shared state. Endpoint assignment
moves the handle. `.share()` deliberately retains the same queue; `copy` and
equality of endpoints or values containing endpoints are rejected.

## Disconnection and scope exit

Dropping the last sender wakes blocked receivers. They drain any buffered messages
before receiving `RecvError.Disconnected`. Dropping the last receiver wakes blocked
senders, returns their pending messages as errors, and drops queued messages.
An empty connected queue yields `RecvError.Empty` only from `recv_nowait`.

Endpoints are also owned context managers. `with receiver as inbox:` moves the
handle into the scope, lends `inbox: &Receiver[T]`, and drops the scoped handle on
every normal exit. The sender form works the same way. Explicitly shared handles
remain independent owners, so dropping one guard need not disconnect the queue.

When consuming only part of a producer's output, place the receiver guard **after**
the producer task manager:

```text
with spawn(producer)? as task, receiver as inbox:
    ...
```

On early `return`, `?`, `break`, or `continue`, cleanup drops the receiver before
joining the producer. Its blocked send can then report disconnection. Without
this ordering, either finish draining or explicitly drop the last receiver before
joining. Automatic joining cannot finish a worker whose communication protocol
still waits for the parent. A worker must handle disconnection and terminate.

## Message eligibility

Messages must have a concrete, wholly owned storable type. The same closed-world
worker checks inspect stored types, concrete callbacks, and destructors. Files,
foreign resources/effects, uncertified indirect callable effects, borrowed
environments, and generator frames are excluded. Checks apply even when a channel
is used only on one thread, so transferring it later cannot change its contract.

Recursive data and sender-based reply paths are supported. A recursive type path
that leads back to a receiving endpoint is rejected: a queue retaining its own
last receiver could prevent destruction from starting. This is a conservative
type check, so it also rejects such a path when a particular instance would not
actually form a cycle. Keep receivers outside queued message cycles.

## Allocation and synchronization

Construction allocates the pair record and one pinned core/ring block. Each ring
slot reserves the complete message layout. Sharing, sending, receiving, waiting,
disconnection, and the error wrappers allocate nothing. Evaluating a message
expression and executing its destructor retain their ordinary effects.

One mutex protects queue state; separate condition variables wait for readability
and writability. `nowait` avoids waiting for queue space/data, but can contend on
the mutex; it is not a lock-free or real-time guarantee. Native Linux futex-backed
std primitives satisfy the current allocation contract. Inline environments move
into receiver-owned storage before their ring slot is reused. User destructors
run outside the mutex.

Zero-capacity rendezvous, timeouts, selection, and a public cancellation token are
not implemented. The [decision record](../proposals/bounded-channels.md) explains
the std/Crossbeam/parking_lot tradeoffs. Native integration tests and runtime Miri
tests cover ownership, failed construction, backpressure, disconnection races,
inline relocation, reentrant cleanup, and guarded early exit.
