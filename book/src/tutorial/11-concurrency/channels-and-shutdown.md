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
    with spawn(job)? as task, receiver as inbox:
        mut total = 0
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

The producer owns the sender. When it finishes, that sender drops. The receiver
drains the remaining messages, then reports disconnection, ending the loop.
`task.join()?` also checks whether the producer returned an application error.

The receiver is a context manager **after** the task manager. If the body exits
early, its receiving handle drops before Plenty joins the producer. The producer
can observe disconnection instead of waiting forever for queue space.

Create additional handles explicitly with `sender.share()` or `receiver.share()`.
They refer to the same queue and need no allocation. Multiple receivers compete:
each message is delivered to one of them. Disconnection occurs only after the
last handle on that side drops, so remember to drop unused handles too.

Channel construction can fail. Its positive capacity fixes the queue's storage
up front; sending and receiving introduce no further allocations. Messages still
use their ordinary types: sending a list moves it, and receiving returns its
ownership. A failed send returns the unsent value inside its error.
