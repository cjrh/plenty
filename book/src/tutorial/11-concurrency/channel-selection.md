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

`select_recv_nowait(&first, &second)` returns `SelectError.Empty` if neither has a
message.

## Wait until a deadline or a stop request

A loop that waits on two channels usually must also stop when asked.
`std.sync.select_recv_before` takes a [deadline](deadlines.md) and a
[cancellation token](cooperative-cancellation.md), and returns
`WaitError.Cancelled` soon after `cancel()`, however long the deadline:

```plenty
import std.sync
from std.sync import WaitError
from std.time import deadline_after, monotonic_now, seconds

def request_stop(stop: &CancellationToken) -> ():
    stop.cancel()

def main() -> Result[(), AllocError | IoError | RangeError | SpawnError[()]]:
    commands, control = channel[str](1)?
    numbers, data = channel[i64](1)?
    stop = CancellationToken()?
    deadline = deadline_after(&monotonic_now(), &seconds(3600))?
    with spawn(request_stop, &stop)?:
        match std.sync.select_recv_before(&control, &data, &deadline, &stop):
            case Ok(Selected.First(command)):
                print(command)?
            case Ok(Selected.Second(number)):
                print(number)?
            case Err(WaitError.Cancelled):
                print("stop requested")?
            case Err(WaitError.TimedOut) | Err(WaitError.Disconnected):
                print("no more work")?
    Ok(())
```

```output
stop requested
```

The senders stay alive, so only a message, the deadline or the token can end
the wait. The token is checked before each attempt and about every 10 ms
while waiting; the [`std.sync` reference](../../design/stdlib/sync.md#typed-deadline-waits)
gives the polling limits. The first channel keeps its priority, as with `select_recv`.

`select_recv_timeout_ms(&first, &second, milliseconds)` is the low-level form.
It may return `SelectError.TimedOut`, but no token can wake it: given an hour,
it keeps a stopping program waiting up to an hour. Use it only where nothing
must interrupt the wait. The [signal lesson](stop-on-signal.md#stop-a-long-wait-on-a-signal)
stops this kind of wait on `SIGTERM`.

Selection skips a drained channel whose senders have all gone away. It reports
`SelectError.Disconnected` only when both channels are drained and disconnected.
The receivers remain yours after every outcome.

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
    while True:
        match select_recv(stopped, data)?:
            case Selected.First(_):
                print("stop")?
                break
            case Selected.Second(number):
                print(number)?
                if number == 1:
                    stop.send(())?
    print(data.recv()?)?
    Ok(())
```

```output
1
stop
2
```

The loop handles the first number, then queues the stop request. The stop
channel is first, so the stop request wins over the number still queued, which
stays in its channel. `case Selected.First(_):`
ignores the unit payload. Generic helpers that bind or take a `T` value, such
as `std.sync.send_before`, do not yet accept `T = ()`; see
[unit messages](../../design/facilities/03-concurrency/channels.md#unit-messages).
