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

def main() -> Result[(), ThreadError | IoError | AllocError | ServerShutdownError]:
    signals = std.signal.listen()?
    mut routes = std.http.server.router[Route]()?
    routes.add("GET", "/", Route.Home)?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    stop = CancellationToken()?
    with spawn(std.signal.cancel_on, signals, stop.share())? as watcher, stop.cancel_on_exit(), spawn(supervisor)? as stopper:
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
`spawn(supervisor)?`; see [cancel on every exit](cooperative-cancellation.md#cancel-on-every-exit).

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
