# Bound your waits

Use a timeout when your program needs control back even if no message arrives.
If a stop request must also end the wait, as in a worker or a long poll, use
`std.sync.recv_before` with a deadline and a cancellation token; the
[shared deadline lesson](deadlines.md) shows it. The millisecond waits here are
the low-level form: no cancellation token can wake them, so a long budget
delays shutdown by that long.

Timeouts are `u64` milliseconds; zero performs an immediate check.

```plenty
def main() -> Result[(), IoError | RecvTimeoutError | SendTimeoutError[i64] | AllocError]:
    sender, receiver = channel[i64](1)?
    match receiver.recv_timeout_ms(0):
        case Ok(value):
            print(value)?
        case Err(RecvTimeoutError.TimedOut):
            print("nothing yet")?
        case Err(RecvTimeoutError.Disconnected):
            print("senders finished")?
    sender.send_timeout_ms(42, 0)?
    print(receiver.recv_timeout_ms(0)?)?
    Ok(())
```

```output
nothing yet
42
```

`send_timeout_ms(value, milliseconds)` returns an unsent value in
`SendTimeoutError[T].TimedOut(value)` or `.Disconnected(value)`. You can retry,
store, or drop it. A timeout does not discard the message.


Timeouts limit waiting, not total wall-clock execution. Lock contention and
scheduling can delay return. A message already available at the check is
received even with a zero budget. To bound several waits by one total budget,
or to let a stop request end a wait, [share a deadline](deadlines.md).
