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
    commands_in, commands = channel[str](1)?
    jobs_in, jobs = channel[i64](1)?
    stop = CancellationToken()?
    deadline = deadline_after(&monotonic_now(), &seconds(3600))?
    with spawn(request_stop, &stop)?:
        match std.sync.select_recv_before(&commands, &jobs, &deadline, &stop):
            case Ok(Selected.First(command)):
                print(command)?
            case Ok(Selected.Second(job)):
                print(job)?
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
the wait. The token is checked before each attempt and at least every 10 ms
while waiting. The first channel keeps its priority, as with `select_recv`.

`select_recv_timeout_ms(&first, &second, milliseconds)` is the low-level form.
It may return `SelectError.TimedOut`, but no token can wake it: given an hour,
it keeps a stopping program waiting up to an hour. Use it only where nothing
must interrupt the wait. The [signal lesson](stop-on-signal.md#stop-a-long-wait-on-a-signal)
stops this kind of wait on `SIGTERM`.

Selection skips a drained channel whose senders have all gone away. It reports
`SelectError.Disconnected` only when both channels are drained and disconnected.
The receivers remain yours after every outcome.
