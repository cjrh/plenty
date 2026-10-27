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

### Recommended patterns

These patterns satisfy the rule on every path, and keep satisfying it when the
block gains early exits. They are the canonical form; the tutorial teaches them
in [avoid waits that hang](../../../tutorial/11-concurrency/wait-habits.md).

1. **Move each endpoint into the worker that uses it**, as a named argument:
   `spawn(produce, sender)?`. A failed named start reports `ThreadError` and
   drops its arguments.
2. **Guard each endpoint the function keeps as the last manager** of the task's
   `with`. Contexts exit in reverse order, so every exit drops it before the
   join:

   ```text
   with spawn(produce, sender)?, receiver as inbox:
       ...
   with spawn(consume, receiver)?, sender as outbox:
       ...
   ```

   Use a new name for the guard binding: `with outbox as outbox:` is a
   duplicate binding. The blocked-join diagnostic suggests an unused name,
   adding a numeric suffix when `inbox` or `outbox` is already bound. Use that
   guard binding for endpoint operations inside the block.

3. **To join explicitly, end the endpoint's guard first.** Open the guard before
   any statement that can leave the block:

   ```text
   with spawn(produce, sender)? as task:
       with receiver as inbox:
           ...
       result = task.join()
   ```

   `drop(receiver)` before the join also satisfies the rule, but only on paths
   that reach it: an earlier `?` still leaves with the receiver alive. Moving
   the receiver into another worker or function likewise ends the binding.
4. **For several producers, give the receiver to a consuming worker** started
   first, and start the producers inside a sender guard:

   ```text
   with spawn(consume, receiver)? as consumer:
       with sender as outbox, spawn(produce, outbox.share())?, spawn(produce, outbox.share())?:
           pass
       total = consumer.join()
   ```

   One receiver guard after several `?` starts is rejected: a failed later
   start joins the earlier tasks before the guard exists. Alternatively, start
   the later producers in a function that takes the receiver by value.
5. **Call [`drop_job()`](jobs.md#job-free-errors) on a closure start** the
   function will not retry: `spawn(job).drop_job()?`.
6. **Wait in a statement of its own**, `result = task.join()`, and move an
   endpoint into a value or return it only after the block.
7. **Start tasks and pools outside lock blocks**; see
   [mutexes](mutex.md#waits-the-compiler-rejects).
8. **Cancel a token that a worker waits for on every exit.** Like an endpoint
   guard, `stop.cancel_on_exit()` goes directly after the task or pool:

   ```text
   with spawn(serve, stop.share())?, stop.cancel_on_exit():
       ...
   ```

   See [cancel on every exit](cancellation.md#cancel-on-every-exit).

Pools follow the same patterns, with `pool.submit(...)` in place of the start;
see [shutdown](executors.md#shutdown-and-live-receivers) and
[result waits](futures.md#result-waits-and-live-receivers).

### What can hold an endpoint

A worker *can hold* a sender that is an argument or capture, a field or element
of one, behind a reference, or a message of a channel it holds. A binding
*can hold* a receiver the same way, except that a local reference is ignored
because its owner is checked instead. A `&Receiver[T]` parameter, or a parameter
that refers to a value holding one, always counts: its owner outlives the call.
The same holder rules apply to receiving workers and surviving senders, including
borrowed sender parameters, aggregates, and concrete callable captures. Other
shared handles count separately, and a drop on only one branch does not release
a handle on the other paths.

Besides live bindings, these values hold the endpoints moved into them:

| Holder | Held through | Rejected example | Fix |
| --- | --- | --- | --- |
| The error that `?` propagates | The function's return, after the exit's joins and pool shutdowns | `with spawn(consumer)? as task, spawn(producer)?:` where the `producer` closure captures the sender | `spawn(producer).drop_job()?`, or a named start `spawn(produce, sender)?` |
| The value a `return` or final expression hands back | The joins and pool shutdowns of every block it leaves | `return Err(sender)` inside the consumer's block | Drop the endpoint in the block, or return it after the block |
| A statement's expression and the value it builds | Until the statement stores, drops, or tests the value | `pair = (sender, task.join())` inside the consumer's block | Join in an earlier statement |
| The final expression of a `match` case used as a value | That case's cleanup | A case whose value is an endpoint, ending a block that the endpoint blocks | Produce the endpoint after the block |

A failed closure start's `SpawnError[F]` retains the unstarted job and every
endpoint it captured, so the first row also applies to an enclosing task block.
Erasing the error into `Failure` drops the job too, as a last resort.

The returned-value and expression checks follow the bindings that the
expression moves, not its type, so `return Ok(())` of `Result[(), Sender[i64]]`
is accepted. A tuple, class, enum payload, `Some`, or call argument that
receives a moved endpoint counts. The check is conservative: an endpoint moved
into a call inside the expression counts even if the call consumes it, so
`return Ok(consume(sender))` is rejected; call `consume(sender)` first. A wait
nested in an expression, such as `task.join()` or `future.result()?`, is
rejected while an operand evaluated before it holds a matching endpoint.

The expression check covers bindings, assignments, unpacking, discarded
expressions, `yield`, `if`, `elif`, and `while` conditions, `for` iterables,
`match` subjects, and `with` managers. A wait inside a called function is seen
only for a [future](futures.md#result-waits-and-live-receivers) or
[pool](executors.md#shutdown-and-live-receivers) passed to the call; a scoped
task cannot be passed to a function.

The diagnostic reports the join or the `with` statement, each binding,
propagated error, returned value, or unfinished expression that holds a matching
endpoint, the early exit that reaches the join, and, for an explicit join, where
the task starts.

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
