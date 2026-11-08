# Bounded channels

`channel[T](capacity: Positive[i64]) -> Result[tuple[Sender[T], Receiver[T]], AllocError]`
constructs a bounded multi-producer, multi-consumer FIFO queue. Capacity must be
a `Positive[i64]`: positive literals convert at compile time; computed counts
use `Positive.try_from(capacity)?`. Zero and negative literals are compile
errors. Size overflow or exhaustion returns `AllocError`. Failed construction exposes no partial queue.

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

> A scoped task whose worker can hold `Sender[T]` cannot be joined while a
> live binding of the joining function can hold `Receiver[T]`.
>
> Likewise, a worker that can hold `Receiver[T]` cannot be joined while a live
> binding of the joining function can hold `Sender[T]`: an empty queue could
> leave the worker waiting forever for a message or the last sender's drop.
>
> Neither applies to a worker that [cannot wait](#which-workers-can-wait).

The rule applies to `task.join()` and to the automatic join on every exit from
the task's `with` block: its end, `return`, `?`, `break`, `continue`, and the
failed start of a later task in the same statement. A task that `task.join()`
has consumed on every path is not joined again at exit. A join is likewise
rejected while the joining function holds a lock on a `Mutex[T]` that the
worker can reach; see [mutexes](mutex.md#waits-the-compiler-rejects).

A binding holds only the fields it still owns. After `wake = ends.wake` and
`drop(ends.inbox)`, `ends` holds neither endpoint, as after destructuring it
with `match`. A field moved out on only some paths still counts on the others,
and assigning the field or the binding again restores the holder. In a loop,
a binding or field assigned in the body still holds its value when the next
iteration reaches a join earlier in the body, unless the body drops it first.

### Recommended patterns

These patterns satisfy the rule on every path, and keep satisfying it when the
block gains early exits. They are the canonical form; the tutorial teaches them
in [avoid waits that hang](../../../tutorial/11-concurrency/wait-habits.md).

1. **Move each endpoint into the worker that uses it**, as a named argument:
   `spawn(produce, sender).drop_job()?`. A failed start returns its owned
   arguments inside `SpawnError`; `drop_job()` drops them before `?` leaves.
2. **Guard each endpoint the function keeps as the last manager** of the task's
   `with`. Contexts exit in reverse order, so every exit drops it before the
   join:

   ```text
   with spawn(produce, sender).drop_job()?, receiver as inbox:
       ...
   with spawn(consume, receiver).drop_job()?, sender as outbox:
       ...
   ```

   Use a new name for the guard binding: `with outbox as outbox:` is a
   duplicate binding. The blocked-join diagnostic suggests an unused name,
   adding a numeric suffix when `inbox` or `outbox` is already bound. Use that
   guard binding for endpoint operations inside the block.

3. **To join explicitly, end the endpoint's guard first.** Open the guard before
   any statement that can leave the block:

   ```text
   with spawn(produce, sender).drop_job()? as task:
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
   with spawn(consume, receiver).drop_job()? as consumer:
       with sender as outbox, spawn(produce, outbox.share()).drop_job()?, spawn(produce, outbox.share()).drop_job()?:
           pass
       total = consumer.join()
   ```

   One receiver guard after several `?` starts is rejected: a failed later
   start joins the earlier tasks before the guard exists. Alternatively, start
   the later producers in a function that takes the receiver by value.
5. **Call [`drop_job()`](jobs.md#job-free-errors) on a start** the
   function will not retry: `spawn(job).drop_job()?` or
   `spawn(produce, sender).drop_job()?`.
6. **Wait in a statement of its own**, `result = task.join()`, and move an
   endpoint into a value or return it only after the block.
7. **Start tasks and pools outside lock blocks**; see
   [mutexes](mutex.md#waits-the-compiler-rejects).
8. **Cancel a token that a worker waits for on every exit.** Like an endpoint
   guard, `stop.cancel_on_exit()` goes directly after the task or pool:

   ```text
   with spawn(serve, stop.share()).drop_job()?, stop.cancel_on_exit():
       ...
   ```

   See [cancel on every exit](cancellation.md#cancel-on-every-exit).

Pools follow the same patterns, with `pool.submit(...)` in place of the start;
see [shutdown](executors.md#shutdown-and-live-receivers) and
[result waits](futures.md#result-waits-and-live-receivers).

### Which workers can wait

Every operation that can block until another thread or process acts counts as
a wait. A worker *can wait* if code it can run performs any operation except:

- computation, memory, and collection operations, including atomics and
  creating queues, tokens, pools, and mutexes;
- `send_nowait`, `recv_nowait`, `select_recv_nowait`, and `submit_nowait`
  on a pool or a [`Submitter`](executors.md#submit-from-workers);
- unlocking a mutex, cancelling a token or future, checking a token or
  future, and starting a worker;
- a sleep, which ends without another thread, and reading the clock,
  environment variables, or the system's random source;
- reading standard input or writing standard output or standard error,
  including `print`; see
  [what the rule does not decide](#what-the-rule-does-not-decide);
- `std.ready.wait_now`, which polls once and returns;
- closing a file, querying or moving its position, reading a file through a
  [`Root`](../../stdlib/fs.md), which refuses pipes and FIFOs, creating a new
  file, and trying a file lock without waiting;
- socket addresses, socket options, and creating or registering with a
  selector.

So a worker waits when it uses a blocking or timed queue operation, a lock, a
pool or future wait, a token wait, a readiness wait, a join, a child process,
any other file, stream, or socket operation, a database statement, or a
foreign call. A timeout bounds the wait, but the bound can be arbitrarily long.
The thread that would end any of these waits could itself be blocked on the
queue the joining function holds. The compiler cannot tell a regular file
from a socket, pipe, or FIFO whose other end that thread holds, and opening a
FIFO waits for its other end. A database statement can wait for a lock another
connection holds. An operation added to the language or the standard library
counts as a wait until it is shown not to block. A pool job that submits
through a `Submitter` also may not wait for what its submitted jobs can reach;
see [waits in submitting jobs](executors.md#waits-in-submitting-jobs).

A worker that uses only the exceptions is joined while the function keeps the
opposite endpoint:

```plenty
def notify(sender: Sender[i64]) -> ():
    match sender.send_nowait(1):
        case Ok(_):
            pass
        case Err(_):
            pass

def main() -> Result[(), AllocError | IoError | RecvError | SpawnError[()]]:
    sender, receiver = channel[i64](1)?
    with spawn(notify, sender).drop_job()?:
        pass
    print(receiver.recv()?)?
    Ok(())
```

```output
1
```

The code a worker can run includes:

- every function it calls, directly or through further calls, closures, sort
  keys, and formatting hooks;
- the `__del__` of every class it can hold, receive, or build, at any depth;
- every worker it starts, which it joins, and every pool job it submits.

A call through a `Callable`, a boxed callable, or a protocol box, a held
boxed value or generator, and a function without a body to inspect, such as
one compiled elsewhere, count as waits.

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
| The error that `?` propagates | The function's return, after the exit's joins and pool shutdowns | `with spawn(consumer)? as task, spawn(producer)?:` where the `producer` closure captures the sender | `spawn(producer).drop_job()?` |
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
- the worker sends only with `send_timeout_ms`, or its blocking sends always
  fit the queue's capacity, or run only on paths the program never takes;
- the worker's only waits are on other queues, locks, tokens, handles, files,
  sockets, databases, or foreign calls that no thread blocked on this queue can
  hold up, such as reads of a regular file;
- another thread also receives from the queue;
- the receiver's caller-side owner would have received.

The receiving-worker rule likewise rejects a sender for an unrelated queue of
the same message type, a worker that only receives with `recv_timeout_ms`, and
a worker whose receives are already satisfied by buffered messages or another
thread. It does not count sends or receives.

The rule is not a deadlock-freedom guarantee. It does not cover:

- an endpoint captured by a `Callable` or boxed callable held by the joining
  function, whose captures have no type;
- endpoints held by the function's callers through bindings it was not passed;
- a worker that retries a `nowait` operation until it succeeds, which waits
  without blocking;
- a worker blocked reading standard input or writing standard output or
  standard error, such as `print` into a full pipe. The other end of a
  standard stream belongs to another process, so no thread blocked on the
  queue can hold it up, unless the program connects its own standard streams
  to a pipe it drains. Counting these streams would make every printing worker
  one that can wait;
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

### Unit messages

`()` can be a message type. `channel[()]`, `Sender[()]`, `Receiver[()]`, every
builtin send, receive, timed and selection operation, and the
[join checks](#joins-and-live-endpoints) accept it, and so do
`std.sync.recv_before` and `std.sync.select_recv_before`. Send `()` when the
message's arrival is the information, such as a stop request or a wakeup.
`case Ok(_):` is the clearest way to match a received unit.

A unit message has no storage. The queue keeps only its count of pending
messages, so construction allocates the fixed queue header whatever the
capacity, and no capacity reports `CapacityOverflow`. Every other message type
takes at least one 16-byte slot per unit of capacity.

Unit is an [ordinary value type](../../language/03-types/index.md), so generic
helpers instantiate with `T = ()`: `std.sync.send_before(&sender, (), &deadline, &cancel)`
returns `Result[(), std.sync.SendBeforeError[()]]`, and a generic relay that
binds a received value and resends it works for a unit channel:

```plenty
import std.sync
import std.time

def relay[T](inbox: &Receiver[T], outbox: &Sender[T]) -> Result[(), RecvError | SendError[T]]:
    value = inbox.recv()?
    outbox.send(value)?
    Ok(())

def main() -> Result[(), AllocError | IoError | RecvError | SendError[()]]:
    cancel = CancellationToken()?
    deadline = std.time.deadline_at(&std.time.monotonic_now())
    tx, rx = channel[()](1)?
    out_tx, out_rx = channel[()](1)?
    print(str.repr(std.sync.send_before(&tx, (), &deadline, &cancel))?)?
    relay(&rx, &out_tx)?
    print(out_rx.recv()?)?
    Ok(())
```

```output
Result[(), std.sync.SendBeforeError[()]].Ok(())
()
```

A unit channel rarely needs a bounded send. Its capacity costs nothing, so a
channel created with a capacity larger than the number of signals it will carry
never blocks in `send(())`. With a small capacity, such as 1,
`send_nowait(())` returning `Full` means a signal is already pending, which is
usually enough. Code that must bound how long it waits for space uses
`std.sync.send_before`.

