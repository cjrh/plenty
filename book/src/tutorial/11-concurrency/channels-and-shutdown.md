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

Dropping the producer's sender lets the receiver drain the queue and report
disconnection. `task.join()?` checks the producer's application result separately.

The receiver is a context manager **after** the task manager. If the body exits
early, its receiving handle drops before Plenty joins the producer. The producer
can observe disconnection instead of waiting forever for queue space.

Create additional handles explicitly with `sender.share()` or `receiver.share()`.
Multiple receivers compete for messages on the same queue:
each message is delivered to one of them. Disconnection occurs only after the
last handle on that side drops, so remember to drop unused handles too.

Channel construction can fail. Its positive capacity fixes the queue's storage
up front; sending and receiving introduce no further allocations. Messages still
use their ordinary types: sending a list moves it, and receiving returns its
ownership. A failed send returns the unsent value inside its error.
