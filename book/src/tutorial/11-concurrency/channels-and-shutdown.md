# Send values between threads

A channel transfers owned messages from senders to receivers. Give it a fixed
capacity to bound queued memory: a sender waits when the queue is full, and a
receiver waits when it is empty.

```plenty
def produce(sender: Sender[i64]) -> Result[(), SendError[i64]]:
    for n in range(5):
        sender.send(n * n)?
    Ok(())

def drain(inbox: &Receiver[i64]) -> i64:
    mut total = 0
    while True:
        match inbox.recv():
            case Ok(value):
                total = total + value
            case Err(_):
                break
    total

def main() -> Result[(), ThreadError | ChannelError | IoError]:
    sender, receiver = channel[i64](2)?
    with spawn(produce, sender)?, receiver as inbox:
        print(drain(inbox))?
    Ok(())
```

```output
30
```

`spawn(produce, sender)?` moves the only sender into the producer. When the
producer returns, its sender drops; the receiver drains the queue, then reports
disconnection, which ends the loop. A blocking `recv` fails only on
disconnection, so `case Err(_)` is enough here.

The receiver guard is the **last** manager in the `with` statement. Managers exit
in reverse order, so however the body exits, the receiver drops before Plenty
joins the producer. A producer waiting for queue space then sees disconnection
instead of waiting forever.

Plenty requires this. A join reached while `receiver` is still alive could wait
forever for a producer blocked on a full queue, so the compiler rejects it:

```plenty-error
def produce(sender: Sender[i64]) -> Result[(), SendError[i64]]:
    for n in range(5):
        sender.send(n * n)?
    Ok(())

def main() -> Result[(), ThreadError | ChannelError | RecvError | IoError]:
    sender, receiver = channel[i64](2)?
    with spawn(produce, sender)?:
        print(receiver.recv()?)?
    Ok(())
```

```error
leaving this `with` block joins a task whose worker holds `Sender[i64]` while `receiver` still holds `Receiver[i64]`
```

The compiler compares message types rather than following each queue, so it
also asks for this order in some programs that could not hang. Write the guard
anyway; it costs nothing.

## Wait for the producer's result

The automatic join above drops the producer's result. To check it with
`task.join()`, finish with the receiver first: put its guard inside the task's
block, before any statement that can leave the block, and join after the guard
ends.

```plenty
def produce(sender: Sender[i64]) -> Result[(), SendError[i64]]:
    for n in range(5):
        sender.send(n * n)?
    Ok(())

def drain(inbox: &Receiver[i64]) -> i64:
    mut total = 0
    while True:
        match inbox.recv():
            case Ok(value):
                total = total + value
            case Err(_):
                break
    total

def main() -> Result[(), ThreadError | ChannelError | IoError | SendError[i64]]:
    sender, receiver = channel[i64](2)?
    with spawn(produce, sender)? as task:
        mut total = 0
        with receiver as inbox:
            total = drain(inbox)
        task.join()?
        print(total)?
    Ok(())
```

```output
30
```

`total` is declared before the inner guard because a binding made inside a
block ends with that block. `task.join()?` propagates the producer's
`SendError[i64]`, if any.

A `?` before the guard leaves the block with the receiver still alive, so this
version is rejected:

```plenty-error
def produce(sender: Sender[i64]) -> Result[(), SendError[i64]]:
    for n in range(5):
        sender.send(n * n)?
    Ok(())

def drain(inbox: &Receiver[i64]) -> i64:
    mut total = 0
    while True:
        match inbox.recv():
            case Ok(value):
                total = total + value
            case Err(_):
                break
    total

def main() -> Result[(), ThreadError | ChannelError | IoError | SendError[i64]]:
    sender, receiver = channel[i64](2)?
    with spawn(produce, sender)? as task:
        print("receiving")?
        mut total = 0
        with receiver as inbox:
            total = drain(inbox)
        task.join()?
        print(total)?
    Ok(())
```

```error
leaving this `with` block joins a task whose worker holds `Sender[i64]` while `receiver` still holds `Receiver[i64]`
`?` leaves the block here
```

Prefer a guard to `drop(receiver)`: a drop protects only the paths that reach
it, and an earlier `?` does not.

## A receiving worker

The same order applies when the worker receives: an empty queue keeps it
waiting while any sender survives. Guard the sender last, so every exit
disconnects the queue before the join:

```plenty
def consume(receiver: Receiver[i64]) -> ():
    match receiver.recv():
        case Ok(_):
            pass
        case Err(_):
            pass

def main() -> Result[(), ThreadError | ChannelError | SendError[i64]]:
    sender, receiver = channel[i64](1)?
    with spawn(consume, receiver)?, sender as outbox:
        outbox.send(42)?
    Ok(())
```

```output
```

Leaving `sender` alive until after this block is rejected, even if the message
is already queued. For an explicit `task.join()`, end an inner sender guard
first, as with the receiver above.

## Ownership and cost

Create additional handles explicitly with `sender.share()` or `receiver.share()`.
Multiple receivers compete for messages on the same queue: each message is
delivered to one of them. Disconnection occurs only after the last handle on
that side drops, so every handle you keep needs a guard or a drop. The
[next lesson](wait-habits.md) shows how to start several producers.

Channel construction can fail. Its positive capacity fixes the queue's storage
up front; sending and receiving introduce no further allocations. Messages still
use their ordinary types: sending a list moves it, and receiving returns its
ownership. A failed send returns the unsent value inside its error.

Starting a named function with arguments reports `ThreadError`, and a failed
start drops the moved arguments, including endpoints. A closure job instead
returns inside `SpawnError`, as in [owned worker jobs](owned-worker-jobs.md);
the next lesson shows why its endpoints matter.
