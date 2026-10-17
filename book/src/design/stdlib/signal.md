# `std.signal`

`import std.signal` lets a program receive `SIGTERM` and `SIGINT` as an
event instead of being ended by them. Supervisors such as systemd and
container runtimes stop a service with `SIGTERM`; a terminal sends `SIGINT`
on Ctrl-C. A program that never calls `listen` keeps the default action:
either signal ends it at once.

The runtime supplies one primitive (opcode 500): it installs the handler and
returns the program's end of a socket pair. Waiting, decoding and the
`cancel_on` helper are checked Plenty in `stdlib/std/signal.plenty`.

## Types and functions

| Item | Contents |
|---|---|
| `Signal` | `Interrupt` (`SIGINT`) or `Terminate` (`SIGTERM`) |
| `SignalError` | `AlreadyListening`, or `Stream(StreamError)` |
| `Signals` | The handle on which caught signals arrive |
| `listen()` | `Result[Signals, SignalError]` |
| `cancel_on(signals: Signals, stop: CancellationToken)` | `Result[Option[Signal], WaitError]` |

| Method | Result |
|---|---|
| `signals.wait(deadline: &Deadline, cancel: &CancellationToken)` | `Result[Signal, WaitError]`: the next signal |
| `signals.received()` | `Result[Option[Signal], WaitError]`: a signal already caught, without waiting |
| `signals.watch(selector: &mut Selector, key: u64)` | `Result[(), WaitError]`: register for reading |
| `signals.unwatch(selector: &mut Selector)` | `Result[(), WaitError]` |
| `signals.close()` | `Result[(), WaitError]` |

`wait` follows the [`std.ready`](ready.md) rules for deadlines and
cancellation. After a [selector](ready.md#selectors) reports the handle,
`received` takes the signal. `WaitError` is the `std.ready` type.

## Behavior

- `listen` catches both signals for the whole process, whichever thread
  they arrive on. Each caught signal is sent as one byte on a socket pair;
  the handler makes no allocation and takes no lock.
- A process listens once. A second `listen` is `AlreadyListening`, even
  after the first handle is closed.
- Only the main thread may call `listen`: a worker that reaches it is
  rejected at compile time, like `std.env.set_current_dir`. See
  [native effects](../facilities/03-concurrency/effects.md). A worker may
  own or borrow the `Signals` handle and wait on it.
- After the first signal, both signals have their default action again, so
  a second one ends the process at once. A drain that takes too long can
  be cut short with a second Ctrl-C.
- Closing or dropping the handle puts the default action back in effect:
  the next signal ends the process as if it had never listened.
- The handler is installed with `SA_RESTART`, so most interrupted system
  calls resume. `std.ready` waits retry `EINTR` against their deadline.
- Child processes started by [`std.process`](process.md) start with the
  default action: `exec` resets caught signals, and the signal mask is not
  changed. A Ctrl-C at a terminal goes to the whole foreground process
  group, children included.
- The handle holds one descriptor, opened close-on-exec. The runtime keeps
  the other end for the life of the process.

## Stopping a server

`cancel_on` waits for a signal and cancels `stop`, returning which signal it
was. If something else cancels `stop` first, it returns `Nothing`. Run it on
its own thread beside [`std.http.server.serve`](http-server.md#shutdown),
whose shutdown and drain begin when `stop` is cancelled:

```text
signals = std.signal.listen()?
stop = CancellationToken()?
with spawn(std.signal.cancel_on, signals, stop.share())? as watcher:
    served = std.http.server.serve(listener, &routes, &handler, &config, &stop)
    stop.cancel()
    watcher.join()?
    report = served?
```

Cancel `stop` before any `?` in the block: leaving the block joins the
watcher, which waits until a signal arrives or `stop` is cancelled. The
[tutorial lesson](../../tutorial/11-concurrency/stop-on-signal.md) runs a
complete program.

## Limits

- x86_64 Linux GNU only; elsewhere `listen` is
  `Stream(StreamError.Io(IoError.Unsupported))`.
- Only `SIGTERM` and `SIGINT`. There is no `SIGHUP` reload, no other
  signal, and no way to catch a signal more than once.
- A library compiled from Plenty does not catch signals unless its code
  calls `listen`; the host's dispositions are its own.
