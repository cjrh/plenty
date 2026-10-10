# Receive from either channel

`select_recv` waits for a message from either of two channels. Their message
types may differ; match the returned `Selected` value to find which one arrived.
Both receivers are borrowed implicitly; explicit `&` arguments also work.

```plenty
def main() -> Result[(), Failure]:
    commands, control = channel[str](1)?
    numbers, data = channel[i64](1)?
    commands.send("stop")?
    numbers.send(42)?
    match select_recv(control, data)?:
        case Selected[str, i64].First(command):
            print(command)?
        case Selected[str, i64].Second(number):
            print(number)?
    print(data.recv()?)?
    Ok(())
```

```output
stop
42
```

When both channels have messages, the first takes priority. The other message
stays in its queue. This can be useful for shutdown commands, but constant traffic
on the first channel can starve the second.

`select_recv_nowait(&first, &second)` returns `SelectError.Empty` if neither has a
message. `select_recv_timeout(&first, &second, milliseconds)` instead waits up
to that budget and may return `SelectError.TimedOut`.

Selection skips a drained channel whose senders have all gone away. It reports
`SelectError.Disconnected` only when both channels are drained and disconnected.
The receivers remain yours after every outcome. The operation and its inline
`Selected` wrapper allocate nothing.
