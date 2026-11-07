# Handle channel backpressure

Use `send_nowait` when a full queue should return control to the caller. Its
`Full` error retains your message, so you can retry it after making space.

```plenty
type MessageError = RecvError | SendError[list[i64]] | AllocError

def main() -> Result[(), AllocError | IoError | MessageError | OverflowError[i64]]:
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
Because they never wait, a worker that uses only `nowait` operations, and no
other operation that can wait, such as a lock, a token wait, file, socket, or
database I/O, or a foreign call, can be joined while you keep the other
endpoint;
see
[which workers can wait](../../design/facilities/03-concurrency/channels.md#which-workers-can-wait).

If you need only part of a stream, the receiver guard handles shutdown:

```plenty
def produce(sender: Sender[i64]) -> Result[(), AllocError | IoError | SendError[i64] | OverflowError[i64]]:
    for n in range(10, 100):
        sender.send(n * n)?
    Ok(())

def first_square() -> Result[i64, SpawnError[()] | AllocError | RecvError]:
    sender, receiver = channel[i64](1)?
    with spawn(produce, sender).drop_job()?, receiver as inbox:
        return Ok(inbox.recv()?)

def main() -> Result[(), SpawnError[()] | AllocError | RecvError | IoError | OverflowError[i64]]:
    print(first_square()?)?
    Ok(())
```

```output
100
```

Returning drops the receiver first. The producer's next send reports
disconnection; its `?` ends the worker. The task scope then joins it and discards
its unclaimed result.

Channel capacity has type `Positive[i64]`. A positive literal converts at
compile time; for configuration read at run time, pass
`Positive.try_from(capacity)?`. Construction then reports only allocation
failure. See [integer domains](../../design/language/03-types/numbers.md#positive-and-non-negative-integers).
