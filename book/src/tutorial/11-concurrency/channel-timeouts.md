# Bound your waits

Use a timeout when your program needs control back even if no message arrives.
Timeouts are `u64` milliseconds; zero performs an immediate check.

```plenty
def main() -> Result[(), IoError | RecvTimeoutError | SendTimeoutError[i64] | ChannelError]:
    sender, receiver = channel[i64](1)?
    match receiver.recv_timeout(0):
        case Ok(value):
            print(value)?
        case Err(RecvTimeoutError.TimedOut):
            print("nothing yet")?
        case Err(RecvTimeoutError.Disconnected):
            print("senders finished")?
    sender.send_timeout(42, 0)?
    print(receiver.recv_timeout(0)?)?
    Ok(())
```

```output
nothing yet
42
```

`send_timeout(value, milliseconds)` returns an unsent value in
`SendTimeoutError[T].TimedOut(value)` or `.Disconnected(value)`. You can retry,
store, or drop it. A timeout does not discard the message.


Timeouts limit waiting, not total wall-clock execution. Lock contention and
scheduling can delay return. A message already available at the check is
received even with a zero budget. To bound several waits by one total budget,
[share a deadline](deadlines.md).
