# Bound your waits

Use a timeout when your program needs control back even if no message arrives.
Timeouts are `u64` milliseconds; zero performs an immediate check.

```plenty
def main() -> Result[(), Failure]:
    sender, receiver = channel[i64](1)?
    match receiver.recv_timeout(0):
        case Ok(value):
            print(value)?
        case Err(error):
            match error:
                case RecvTimeoutError.TimedOut:
                    print("nothing yet")?
                case RecvTimeoutError.Disconnected:
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

For a future, `future.wait_timeout(milliseconds)` returns a boolean: true means
completed or cancelled. This only observes the future; call `result()` afterward
to take its result. A false return leaves the job running or queued.

When combining a token with a receive loop, check `is_cancelled()`, perform a
bounded receive, then check again on the next iteration. Use a budget appropriate
to the work; a zero-timeout retry loop can consume a CPU while idle.

Timeouts limit waiting, not total wall-clock execution. Lock contention and
scheduling can delay return. A message already available at the check is
received even with a zero budget.
