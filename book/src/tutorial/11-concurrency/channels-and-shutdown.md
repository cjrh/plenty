# Send values between threads

A channel transfers owned messages from senders to receivers. Give it a fixed
capacity to bound queued memory: a sender waits when the queue is full, and a
receiver waits when it is empty.

```plenty
def produce(sender: Sender[i64]) -> Result[(), Failure]:
    for n in range(5):
        sender.send(n * n)?
    Ok(())

def main() -> Result[(), Failure]:
    sender, receiver = channel[i64](2)?
    job = def once [sender]() -> Result[(), Failure]:
        produce(sender)
    with spawn(job)? as task:
        mut total = 0
        with receiver as inbox:
            while True:
                match inbox.recv():
                    case Ok(value):
                        total = total + value
                    case Err(_):
                        break
        task.join()?
        print(total)?
    Ok(())
```

```output
30
```

Dropping the producer's sender lets the receiver drain the queue and report
disconnection. `task.join()?` checks the producer's application result separately.

The receiver is a context manager **inside** the task's block, and its block ends
before `task.join()`. However the body exits, the receiving handle drops before
Plenty joins the producer. The producer can observe disconnection instead of
waiting forever for queue space.

Plenty requires this order. A producer waiting for queue space is released only
when the last receiver drops, so a join reached while `receiver` is still alive
could wait forever. The compiler rejects it:

```plenty-error
def produce(sender: Sender[i64]) -> Result[(), Failure]:
    for n in range(5):
        sender.send(n * n)?
    Ok(())

def main() -> Result[(), Failure]:
    sender, receiver = channel[i64](2)?
    with spawn(produce, sender)?:
        print(receiver.recv()?)?
    Ok(())
```

```error
leaving this `with` block joins a task whose worker holds `Sender[i64]` while `receiver` still holds `Receiver[i64]`
```

When you do not need the producer's result, write both managers in one
statement, the receiver last: `with spawn(produce, sender)?, receiver as inbox:`.
Managers exit in reverse order, so the receiver drops first. The compiler
compares message types rather than following each queue, so it also asks for
this order in some programs that could not hang.

Create additional handles explicitly with `sender.share()` or `receiver.share()`.
Multiple receivers compete for messages on the same queue:
each message is delivered to one of them. Disconnection occurs only after the
last handle on that side drops, so remember to drop unused handles too.

The same ordering applies when the worker receives: an empty queue keeps it
waiting while any sender survives. Guard the sender after the task so every exit
disconnects the queue before joining:

```plenty
def consume(receiver: Receiver[i64]) -> ():
    match receiver.recv():
        case Ok(_):
            pass
        case Err(_):
            pass

def main() -> Result[(), Failure]:
    sender, receiver = channel[i64](1)?
    with spawn(consume, receiver)?, sender as outbox:
        outbox.send(42)?
    Ok(())
```

```output
```

Leaving `sender` alive until after this block is rejected, even if the message
is already queued. For an explicit `task.join()`, end an inner sender guard or
drop every sender handle first. A drop after an earlier `?` cannot protect that
early exit.

Channel construction can fail. Its positive capacity fixes the queue's storage
up front; sending and receiving introduce no further allocations. Messages still
use their ordinary types: sending a list moves it, and receiving returns its
ownership. A failed send returns the unsent value inside its error.
