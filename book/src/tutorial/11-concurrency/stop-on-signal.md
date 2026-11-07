# Stop a server on a signal

A supervisor such as systemd, a container runtime or a shell stops a program
with `SIGTERM`, or `SIGINT` for Ctrl-C. By default either signal ends a
Plenty program at once, cutting off responses in flight.
[`std.signal`](../../design/stdlib/signal.md) turns them into an event, so
the [HTTP server](http-server.md) can drain instead:

```plenty
import std.http
import std.http.server
import std.net
import std.process
import std.ready
import std.signal
from std.http.server import Exchange
from std.net import TcpStream
from std.signal import Signal
from std.sync import CancellationToken

pub enum Route:
    Home

pub class Hello:
    pub def handle(self, route: &Route, exchange: &mut Exchange[TcpStream]) -> Result[(), std.http.HttpError]:
        exchange.send_text(200, "hello\n")?
        Ok(())

# Plays the supervisor: a shell sends SIGTERM to its parent, this program.
def supervisor() -> Result[(), AllocError | IoError]:
    mut command = std.process.shell("kill -TERM $PPID")?
    command.spawn()?.wait()?
    Ok(())

type ServerShutdownError = std.signal.SignalError | std.http.server.ServerError | std.net.NetError | std.ready.WaitError

def main() -> Result[(), SpawnError[()] | IoError | AllocError | ServerShutdownError]:
    signals = std.signal.listen()?
    mut routes = std.http.server.router[Route]()?
    routes.add("GET", "/", Route.Home)?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    stop = CancellationToken()?
    with spawn(std.signal.cancel_on, signals, stop.share()).drop_job()? as watcher, stop.cancel_on_exit(), spawn(supervisor).drop_job()? as stopper:
        served = std.http.server.serve(listener, &routes, &Hello(), &std.http.server.config(), &stop)
        stop.cancel()
        stopper.join()?
        match watcher.join()?:
            case Some(Signal.Terminate):
                print("stopping on SIGTERM")?
            case Some(Signal.Interrupt):
                print("stopping on SIGINT")?
            case Nothing:
                print("stopped without a signal")?
        print(served?.requests)?
    Ok(())
```
```output
stopping on SIGTERM
0
```

`std.signal.listen()` catches both signals for the whole process and returns
a `Signals` handle on which they arrive. Call it on the main thread, before
the work it should stop; a worker may not call it, and a process listens only
once.

`cancel_on` runs on its own thread. It waits for a signal and cancels `stop`,
which begins the server's [shutdown](../../design/stdlib/http-server.md#shutdown):
`serve` stops accepting, finishes the requests it has within the drain
timeout, and returns its `Report`. If `serve` returns for another reason,
`stop.cancel()` ends the watcher instead, and it returns `Nothing`.

The watcher waits until a signal arrives or `stop` is cancelled, and the block
joins it as it exits. `stop.cancel_on_exit()` directly after the watcher
cancels `stop` before that join on every exit, including a failed
`spawn(supervisor).drop_job()?`; see [cancel on every exit](cooperative-cancellation.md#cancel-on-every-exit).

After the first signal both signals have their default action again, so a
second Ctrl-C or `SIGTERM` ends a slow drain at once. Closing or dropping
the `Signals` handle restores the default action too. Child processes
started by [`std.process`](../08-resources/28-environment-processes.md)
keep the default action.

A program that is not a server can wait on the handle directly with
`signals.wait(&deadline, &cancel)`, or register it on a
[selector](tcp-sockets.md#wait-on-many-connections) with
`signals.watch(&mut selector, key)` and take the signal with
`signals.received()`.

## Stop a long wait on a signal

A worker loop that waits for messages, such as a long poll, stops the same
way. Give its wait a deadline and the same token, through
[`std.sync.select_recv_before`](channel-selection.md#wait-until-a-deadline-or-a-stop-request)
or `recv_before`. Here the wait may last a minute, but `SIGTERM` ends it at
once:

```plenty
import std.process
import std.ready
import std.signal
import std.sync
from std.signal import Signal
from std.sync import CancellationToken
from std.time import deadline_after, monotonic_now, seconds

def supervisor() -> Result[(), AllocError | IoError]:
    mut command = std.process.shell("kill -TERM $PPID")?
    command.spawn()?.wait()?
    Ok(())

type ShutdownError = std.signal.SignalError | std.ready.WaitError | RangeError

def main() -> Result[(), SpawnError[()] | IoError | AllocError | ShutdownError]:
    signals = std.signal.listen()?
    commands_in, commands = channel[str](4)?
    jobs_in, jobs = channel[str](4)?
    stop = CancellationToken()?
    start = monotonic_now()
    deadline = deadline_after(&start, &seconds(60))?
    with spawn(std.signal.cancel_on, signals, stop.share()).drop_job()? as watcher, stop.cancel_on_exit(), spawn(supervisor).drop_job()? as stopper:
        match std.sync.select_recv_before(&commands, &jobs, &deadline, &stop):
            case Ok(Selected.First(command)):
                print(command)?
            case Ok(Selected.Second(job)):
                print(job)?
            case Err(std.sync.WaitError.Cancelled):
                print("stop requested")?
            case Err(std.sync.WaitError.TimedOut) | Err(std.sync.WaitError.Disconnected):
                print("no more work")?
        stop.cancel()
        stopper.join()?
        match watcher.join()?:
            case Some(Signal.Terminate):
                print("on SIGTERM")?
            case Some(Signal.Interrupt):
                print("on SIGINT")?
            case Nothing:
                print("without a signal")?
    print(monotonic_now().duration_since(&start)?.as_seconds() < 1u64)?
    Ok(())
```
```output
stop requested
on SIGTERM
True
```

The watcher cancels `stop`, and `select_recv_before` sees it within about one
10 ms polling slice. The program stops in well under a second, as the last line
checks. Two error types share the name `WaitError`: `std.sync.WaitError` comes
from the selection, and `std.ready.WaitError` from `cancel_on`, through
`watcher.join()?`. The example names both by module.

Do not write this wait as `select_recv_timeout_ms(&commands, &jobs,
seconds(60).as_timeout_ms())`. That low-level wait takes no token, so after
`SIGTERM` the program would keep waiting for the rest of the minute, or until a
message arrived. A real long poll may wait much longer. The same holds for `recv_timeout_ms`, `send_timeout_ms` and a
future's `wait_timeout_ms`; the
[timed wait reference](../../design/facilities/03-concurrency/timed-selection.md)
lists them.
