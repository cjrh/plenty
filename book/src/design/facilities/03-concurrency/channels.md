# Bounded channels

`channel[T](capacity: u64) -> Result[tuple[Sender[T], Receiver[T]], ChannelError]`
constructs a bounded multi-producer, multi-consumer FIFO queue. Capacity must be
positive: zero returns `InvalidCapacity`; size overflow or exhaustion returns
`Allocation(AllocError)`. Failed construction exposes no partial queue.

| Operation | Result | Behavior |
| --- | --- | --- |
| `sender.send(value)` | `Result[(), SendError[T]]` | Wait for space or receiver disconnection |
| `sender.send_nowait(value)` | Same | Return `Full(value)` if no space |
| `receiver.recv()` | `Result[T, RecvError]` | Wait for a message or sender disconnection |
| `receiver.recv_nowait()` | Same | Return `Empty` if no message is ready |
| `endpoint.share()` | Same endpoint type | Create another owner of the queue |

Methods observe endpoints; `mut` is unnecessary. Endpoint assignment moves;
`share()` explicitly retains the queue. Copy and equality are unsupported.
Sending moves affine messages. `SendError[T].Disconnected(value)` and
`Full(value)` own the complete unsent value; blocking send never returns Full.
Receiving transfers one message to one receiver. Shared receivers compete rather
than broadcast. Accepted messages follow insertion order, but concurrent senders
and waiters have no predetermined or strictly fair order.

Construction uses one allocation. Sharing, send, receive, waits, disconnection,
and error handling allocate nothing. `nowait` avoids waiting for queue space/data
but can contend on synchronization; it does not guarantee immediate execution.
Timed operations and receive selection are described [separately](timed-selection.md).

## Disconnection and cleanup

Dropping the last sender wakes receivers, which drain buffered messages before
returning `RecvError.Disconnected`. Dropping the last receiver wakes senders,
returns their pending messages as errors, and drops queued messages.

Endpoints are owned context managers. `with receiver as inbox:` moves the handle
into the scope, lends `&Receiver[T]`, and drops that owner on every normal exit.
Other shared handles remain independent owners.

Workers must handle disconnection and terminate.

## Joins and live endpoints

A worker waiting for queue space is released only by a receive or by the drop of
the last receiver. A join that waits for such a worker while the joining
function keeps a receiver alive, and does not receive, never returns. The
compiler therefore rejects a join that could wait this way:

> A scoped task whose worker can hold `Sender[T]` cannot be joined while a live
> binding of the joining function can hold `Receiver[T]`.
>
> Likewise, a worker that can hold `Receiver[T]` cannot be joined while a live
> binding of the joining function can hold `Sender[T]`: an empty queue could
> leave the worker waiting forever for a message or the last sender's drop.

The rule applies to `task.join()` and to the automatic join on every exit from
the task's `with` block: its end, `return`, `?`, `break`, `continue`, and the
failed start of a later task in the same statement. A task that `task.join()`
has consumed on every path is not joined again at exit. A join is likewise
rejected while the joining function holds a lock on a `Mutex[T]` that the
worker can reach; see [mutexes](mutex.md#waits-the-compiler-rejects).

A worker *can hold* a sender that is an argument or capture, a field or element
of one, behind a reference, or a message of a channel it holds. A binding
*can hold* a receiver the same way, except that a local reference is ignored
because its owner is checked instead. A `&Receiver[T]` parameter, or a parameter
that refers to a value holding one, always counts: its owner outlives the call.
The same holder rules apply to receiving workers and surviving senders, including
borrowed sender parameters, aggregates, and concrete callable captures.

Place the receiver guard directly after the task manager. Contexts exit in
reverse order, so every exit drops the receiver before joining the producer:

```text
with spawn(producer)?, receiver as inbox:
    ...
```

To claim the producer's result, end the guard before joining:

```text
with spawn(producer)? as task:
    with receiver as inbox:
        ...
    result = task.join()
```

`drop(receiver)` before the join also satisfies the rule, but only on paths that
reach it: an earlier `?` in the block still leaves with the receiver alive.
Moving the receiver into another worker or function likewise ends the binding.
Several producers that start with `?` cannot share one guard in the same
statement, because a failed later start joins the earlier ones before the guard
exists. Give the receiver to a consuming worker, or start the later producers in
a function that takes the receiver by value.

For a receiving worker, drop or guard every matching sender before joining.
Write `with spawn(consumer)?, sender as outbox:` for an automatic join, or end
the sender's inner guard before `task.join()`. Other shared sender handles still
count; a drop on only one branch does not release a handle on other paths. A
failed later task start also joins earlier tasks before any later guard exists.

The error that `?` propagates is a holder too: the function keeps it until it
returns, after the exit's joins and pool shutdowns. A failed consuming start's
`SpawnError[F]` retains the unstarted job and every endpoint it captured, so a
consumer started earlier in the same statement, or in an enclosing task block,
is rejected when the job captures the consumer's sender:

```text
with spawn(consumer)? as task, spawn(producer)?:  # producer captures the sender
    task.join()?
```

Erasing the error into `Failure` drops the job before the joins. To keep a
precise error, pass the endpoint to `spawn` as a named argument,
`spawn(produce, sender)?`: a failed named start reports `ThreadError` and drops
its arguments first.

The diagnostic reports the join or the `with` statement, each binding or
propagated error that holds a matching endpoint, the early exit that reaches the join, and, for an explicit
join, where the task starts.

Pool shutdown checks sending jobs against surviving receivers.
See [shutdown and live receivers](executors.md#shutdown-and-live-receivers).

### What the rule does not decide

Endpoints are matched by message type, not by queue, and the compiler does not
count messages. These safe programs are rejected:

- the parent receives everything the worker sends, or drains to disconnection,
  through a receiver that is still alive at the join;
- the live receiver belongs to a different queue of the same message type;
- the worker sends only with `send_nowait` or `send_timeout`, or its sends
  always fit the queue's capacity;
- another thread also receives from the queue;
- the receiver's caller-side owner would have received.

The receiving-worker rule likewise rejects a sender for an unrelated queue of
the same message type, a worker that only receives with `recv_nowait` or
`recv_timeout`, and a worker whose receives are already satisfied by buffered
messages or another thread. It does not count sends or receives.

The rule is not a deadlock-freedom guarantee. It does not cover:

- an endpoint captured by a `Callable` or boxed callable held by the joining
  function, whose captures have no type;
- endpoints held by the function's callers through bindings it was not passed;
- a worker blocked for any other reason.

Dropping or guarding the surviving endpoints removes the rejection. Workers
must still handle disconnection and terminate.

## Messages

Messages must be concrete wholly owned storable values satisfying the
[worker checks](index.md), even for a channel used on one thread. Recursive data
and sender-based reply paths are supported, but recursive message-type paths
back to a receiver are rejected: a queue retaining its last receiver could
prevent destruction. This conservative check also rejects noncyclic instances of
such a type. Zero-capacity rendezvous is unsupported.
