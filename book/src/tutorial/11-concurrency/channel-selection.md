# Receive from either channel

`select_recv` waits for a message from either of two channels. Their message
types may differ; match the returned `Selected` value to find which one arrived.
Both receivers are borrowed implicitly; explicit `&` arguments also work.

```plenty
type ChannelSelectionError = RecvError | SelectError | SendError[i64] | SendError[str] | AllocError

def main() -> Result[(), IoError | ChannelSelectionError]:
    commands, control = channel[str](1)?
    numbers, data = channel[i64](1)?
    commands.send("stop")?
    numbers.send(42)?
    match select_recv(control, data)?:
        case Selected.First(command):
            print(command)?
        case Selected.Second(number):
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

## Signal with a unit message

When the arrival of a message is all it says, send `()`. A `channel[()]`
stores no message data, only a count, so a stop request needs no placeholder
value such as `True`:

```plenty
type StopError = RecvError | SelectError | SendError[i64] | SendError[()] | AllocError

def main() -> Result[(), IoError | StopError]:
    stop, stopped = channel[()](1)?
    numbers, data = channel[i64](4)?
    numbers.send(1)?
    numbers.send(2)?
    stop.send(())?
    while True:
        match select_recv(stopped, data)?:
            case Selected.First(_):
                print("stop")?
                break
            case Selected.Second(number):
                print(number)?
    print(data.recv()?)?
    Ok(())
```

```output
stop
1
```

Match a unit message with `_`. Because the stop channel is first, the stop
request wins even though both numbers were sent before it; they stay queued.

`select_recv_nowait(&first, &second)` returns `SelectError.Empty` if neither has a
message. `select_recv_timeout_ms(&first, &second, milliseconds)` instead waits up
to that budget and may return `SelectError.TimedOut`.

Selection skips a drained channel whose senders have all gone away. It reports
`SelectError.Disconnected` only when both channels are drained and disconnected.
The receivers remain yours after every outcome.
