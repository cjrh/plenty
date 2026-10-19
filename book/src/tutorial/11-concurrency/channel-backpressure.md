# Handle channel backpressure

Use `send_nowait` when a full queue should return control to the caller. Its
`Full` error retains your message, so you can retry it after making space.

```plenty
def main() -> Result[(), AllocError | IoError | RecvError | SendError[list[i64]] | ChannelError]:
    sender, receiver = channel[list[i64]](1)?
    sender.send([10]?)?
    match sender.send_nowait([20, 30]?):
        case Ok(_):
            pass
        case Err(SendError.Full(message)):
            print(receiver.recv()?)?
            sender.send(message)?
        case Err(SendError.Disconnected(message)):
            print(message)?
    print(receiver.recv()?)?
    drop(sender)
    print(str.repr(receiver.recv())?)?
    Ok(())
```
```output
[10]
[20, 30]
Result[list[i64], RecvError].Err(RecvError.Disconnected)
```

`recv_nowait()` similarly reports `RecvError.Empty` when the channel is connected
but has no message ready. Neither `nowait` operation waits for the queue to change
state, though both synchronize access to the queue.

If you need only part of a stream, the receiver guard handles shutdown:

```plenty
def produce(sender: Sender[i64]) -> Result[(), AllocError | IoError | SendError[i64]]:
    for n in range(10, 100):
        sender.send(n * n)?
    Ok(())

def first_square() -> Result[i64, ThreadError | ChannelError | RecvError]:
    sender, receiver = channel[i64](1)?
    with spawn(produce, sender)?, receiver as inbox:
        return Ok(inbox.recv()?)

def main() -> Result[(), ThreadError | ChannelError | RecvError | IoError]:
    print(first_square()?)?
    Ok(())
```

```output
100
```

Returning drops the receiver first. The producer's next send reports
disconnection; its `?` ends the worker. The task scope then joins it and discards
its unclaimed result.
