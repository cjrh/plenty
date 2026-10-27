# `std.actor.net`

TCP for [`std.actor`](actor.md): a listener that starts one actor per
connection, a connection with bounded buffers and deadlines, and connect and
name lookup that do not block the stage. A connection's buffers are allocated
in its actor's heap when it is admitted; handling readiness and timers
allocates nothing. A trap in a connection actor closes that connection only.

Supported target: x86-64 Linux, as for [`std.actor`](actor.md).

A server echoes one connection, and a client actor dials it, sends a request
and prints the reply:

```plenty
from std.actor import Address, Context, Mail, TimerError, heaps_live, stage
from std.actor.net import Admission, Connection, Dialing, Limits, Listener, connection, dial, listener
from std.bytes import BytesError
from std.net import NetError, SocketAddress, listen, loopback_v4
from std.ready import WaitError
from std.sync import CancellationToken
from std.time import deadline_after, monotonic_now, seconds

type Fault = NetError | BytesError | TimerError | IoError

pub enum Step:
    Waiting(SocketAddress)
    Dialing(Dialing)
    Talking(Connection)

# Connects, sends one request, and prints the reply.
pub class Client:
    pub step: Step

    pub def started(self: &mut Client, context: &mut Context[i64, Fault]) -> Result[(), Fault]:
        match &self.step:
            case Step.Waiting(address):
                self.step = Step.Dialing(dial(address, &seconds(5), context)?)
            case _:
                pass
        Ok(())

    pub def receive(self: &mut Client, mail: &Mail[i64, Fault], context: &mut Context[i64, Fault]) -> Result[(), Fault]:
        match &mut self.step:
            case Step.Dialing(attempt):
                match attempt.handle(mail, context)?:
                    case Some(stream):
                        mut call = connection(stream, &Limits(64, 64, seconds(5), seconds(5)))?
                        call.start(context)?
                        call.write(b"hello, actors", 0, 13, context)?
                        call.shutdown_write(context)?
                        self.step = Step.Talking(call)
                    case Nothing:
                        pass
            case Step.Talking(call):
                events = call.handle(mail, context)?
                if events.ended:
                    print("reply", call.input().to_text_range(call.input_start(), call.input_stop())?)?
                    call.close(context)?
                    context.stop()
            case Step.Waiting(_):
                pass
        Ok(())

pub enum Node:
    Serving(Listener)
    Echoing(Connection)
    Calling(Client)

    pub def started(self: &mut Node, context: &mut Context[i64, Fault]) -> Result[(), Fault]:
        match self:
            case Node.Serving(server):
                server.start(context)?
            case Node.Echoing(echo):
                echo.start(context)?
            case Node.Calling(client):
                client.started(context)?
        Ok(())

    pub def receive(self: &mut Node, mail: Mail[i64, Fault], context: &mut Context[i64, Fault]) -> Result[(), Fault]:
        match self:
            case Node.Serving(server):
                server.handle(&mail, context)?
                match mail:
                    # This server handles one connection.
                    case Mail.Down(_, _):
                        context.stop()
                    case _:
                        pass
            case Node.Echoing(echo):
                events = echo.handle(&mail, context)?
                if echo.available() > 0:
                    echo.forward(echo.available(), context)?
                if events.ended and echo.available() == 0:
                    echo.shutdown_write(context)?
                if not echo.reading() and echo.pending() == 0:
                    context.stop()
            case Node.Calling(client):
                client.receive(&mail, context)?
        Ok(())

    # Builds each connection's actor, with its buffers in the child's heap.
    pub def spawn(self, tag: i64, child: Address, context: &mut Context[i64, Fault]) -> Result[Option[Node], Fault]:
        match self:
            case Node.Serving(server):
                match server.accept()?:
                    case Some(echo):
                        Ok(Some(Node.Echoing(echo)))
                    case Nothing:
                        Ok(Nothing)
            case _:
                Ok(Nothing)

def main() -> Result[(), AllocError | IoError | WaitError | NetError]:
    socket = listen(&loopback_v4(0u16), 16)?
    address = socket.local_address()?
    limits = Limits(4096, 4096, seconds(30), seconds(30))
    server = listener(socket, limits, 100, 65536, 0, Admission.Refuse)?
    mut actors = stage[Node, i64, Fault](8, 4, 8)?
    actors.add_timers(8)?
    actors.start(Node.Serving(server), 65536).unwrap()
    actors.start(Node.Calling(Client(Step.Waiting(address))), 65536).unwrap()
    actors.run(&deadline_after(&monotonic_now(), &seconds(10)).unwrap(), &CancellationToken()?)?
    stats = actors.stats()
    print(stats.started, stats.stopped, heaps_live())?
    Ok(())
```
```output
reply hello, actors
3 3 0
```

The repository's `examples/actor_echo.plenty` is a longer echo server: a
connection whose first byte is `X` traps, and only that connection closes.

## Listening

A `Listener` belongs to one actor, which watches the socket and starts a child
actor for each connection. Call `start` from the actor's `started` and pass
every mail to `handle`. On readiness, `handle` spawns one child with the
listener's `tag` and heap limit; the actor's [spawn hook](actor.md#the-spawn-hook)
then calls `accept`, which accepts the connection and allocates its buffers in
the child's heap. A child's `Down` frees its place.

At most `capacity` connections are live. A connection that arrives at capacity,
or when the stage has no free slot or heap, is handled by `Admission`:

| `Admission` | At capacity |
|---|---|
| `Pause` | Stops accepting until a connection ends. Arrivals wait in the kernel's backlog; once it is full the kernel drops further attempts, so peers wait on their own connect timeouts |
| `Refuse` | Accepts and closes the connection at once, so the peer sees it end without data. `refused()` counts them |

Pause suits short connections and load bursts; Refuse tells peers at once,
which suits clients that retry elsewhere. At the descriptor limit
(`NetError.HandleLimit`), both stop accepting until a connection ends.

## Connections

A `Connection` owns its stream and two buffers sized by `Limits`. Call `start`
from the actor's `started`, and pass every mail to `handle`; it reads, writes
and checks deadlines, and returns what happened as `Events`:

| Field | Meaning |
|---|---|
| `mine` | The mail was about this connection |
| `received` | Bytes this mail added to the input |
| `ended` | The peer finished sending; no more input will arrive |
| `writable` | Output space freed after `write` accepted less than it was offered |
| `drained` | All output has been sent |
| `expired` | `Option[Expired]`: a deadline passed; see [Deadlines](#deadlines) |

Readiness mail does not say which handle is ready, so `handle` tries the
socket in the directions it is watching. Each mail reads at most once and
writes at most four times, so a fast connection cannot hold the stage.
Readiness is level-triggered, so unread input brings another `Mail.Ready`.

### Reading

Received bytes are `input()[input_start():input_stop()]`, read in place. Call
`consume(count)` once they are handled. When the input buffer is full the
connection stops reading, so the kernel's buffers fill and the peer's sends
block in turn; `consume` resumes reading. A request larger than `Limits.input`
must be handled in parts or refused.

### Writing and back-pressure

`write(source, start, stop)` copies as much of `source[start:stop]` as fits
into the output buffer, sends what the socket accepts at once, and returns the
count accepted. A short count is back-pressure: keep the rest and offer it
again when `Events.writable` is set. `forward(count)` moves received input to
the output, for echoing and relaying. `pending()` and `space()` report the
output buffer.

### Closing

`shutdown_write` ends output once pending bytes are sent; the peer then reads
end of input, and reading continues. `close` closes the socket at once,
discarding unsent output. An actor whose connection is done stops with
`Context.stop`; dropping the connection closes the socket. A reset or other
socket failure is returned by `handle` as an error, and returning it ends the
actor with `Exit.Failed`.

## Deadlines

| Deadline | Set by | Ends when |
|---|---|---|
| Idle | `Limits.idle` | No bytes moved either way for this long |
| Stall | `Limits.stall` | Pending output made no progress for this long, such as a peer that stopped reading |
| Read | `read_within(&span)` | Input did not finish before `clear_read_deadline`; bounds how long a request may take to arrive |

A zero duration turns a `Limits` deadline off. `handle` reports a passed
deadline once in `Events.expired`; the actor decides what to do, usually
`close` and `stop`. Idle and stall then restart from now, and the read deadline
is cleared.

Each connection uses one [actor timer](actor.md#timers), re-armed only when it
fires or a sooner deadline starts, so moving data does not create or cancel
timers. A stage needs `add_timers` with one record per connection that has a
deadline, and one per `dial` or `lookup` with a timeout.

## Outbound connections and lookup

`dial(&address, &timeout, context)` starts a nonblocking connect and returns a
`Dialing`; `lookup(host, port, &timeout, context)` starts a name lookup and
returns a `Resolving`. Pass every mail to their `handle`, which returns
`Nothing` until the result is ready, then the stream or the addresses, or the
error. A zero timeout waits indefinitely; a passed one is `NetError.TimedOut`.
`abandon` stops waiting. Make a `Connection` from a dialled stream with
`connection(stream, &limits)`.

The lookup runs the system resolver on a helper thread, as
[`std.net.resolve`](net.md#name-lookup) does, so the stage keeps running. Its
request and reply are allocated outside the actor's heap, so a trap in the
actor while the lookup runs does not affect the helper thread. At most 32
lookups run at once in a process; another fails with `TryAgain`. Calling
`std.net.resolve` or `connect` in a handler also works, but blocks every actor
on the stage until it returns.

## Memory

A connection actor's heap holds its node, both buffers and 192 bytes of other
state, measured with `Context.heap_used()` after `started`: 2240 bytes with
1 KiB buffers and 8384 with 4 KiB buffers. The heap limit passed to
`listener` reserves address space; memory is committed as the heap is used.
The stage's slot, mailbox and timer records are allocated by `stage` and
`add_timers`. Admission allocates the buffers; nothing else on the
connection's path allocates. `lookup` allocates its result list in the actor's
heap.

## Exports

| Export | Contract |
|---|---|
| `Limits(input, output, idle, stall)` | Buffer sizes in bytes and deadlines; see [Deadlines](#deadlines) |
| `listener(socket, limits, capacity, heap, tag, admission)` | `Result[Listener, NetError]`; `InvalidInput` for a size, capacity or heap below 1 |
| `Listener.start(context)` | `Result[(), NetError]` |
| `Listener.handle(&mail, context)` | `Result[bool, NetError]`: `True` for mail the listener used |
| `Listener.accept()` | `Result[Option[Connection], NetError \| BytesError]`: in the spawn hook; `Nothing` when no connection waits, and the hook should return `Nothing` |
| `Listener.live()`, `refused()`, `tag()`, `local_address()` | Admitted connections not yet ended, connections refused, the child tag, the bound address |
| `connection(stream, &limits)` | `Result[Connection, BytesError]`: buffers in the current heap |
| `Connection.start(context)` | `Result[(), NetError \| TimerError]` |
| `Connection.handle(&mail, context)` | `Result[Events, NetError \| TimerError]` |
| `Connection.input()`, `input_start()`, `input_stop()`, `available()` | The input buffer and the unconsumed range |
| `Connection.consume(count, context)` | `Result[(), NetError \| TimerError]` |
| `Connection.write(source, start, stop, context)` | `Result[i64, NetError \| TimerError]`: bytes accepted; `Stream(StreamError.Io(IoError.Closed))` after `shutdown_write` or `close`, and `InvalidInput` for a range outside `source` |
| `Connection.forward(count, context)` | `Result[i64, NetError \| TimerError]`: bytes moved from input to output |
| `Connection.pending()`, `space()` | Unsent output bytes; bytes `write` would accept now |
| `Connection.reserve(wanted)`, `output()`, `output_stop()`, `commit(count, context)` | Produce output in place: `reserve` compacts and returns `Result[i64, NetError]`, the room up to `wanted`; write into `output()` from `output_stop()`; `commit` sends the `count` bytes written. For layers such as [`std.actor.tls`](actor-tls.md) |
| `Connection.acknowledge_now()` | Acknowledge received data at once (`TCP_QUICKACK`), as `TcpStream.acknowledge_now` in [`std.net`](net.md); best effort |
| `Connection.read_within(&span, context)`, `clear_read_deadline()` | Set or clear the read deadline |
| `Connection.shutdown_write(context)`, `close(context)` | End output after pending bytes; close at once |
| `Connection.reading()`, `closed()`, `stream()` | Whether input may still arrive; whether closed; the stream |
| `dial(&address, &timeout, context)` | `Result[Dialing, NetError \| TimerError]` |
| `Dialing.handle(&mail, context)` | `Result[Option[TcpStream], NetError]`: `Refused`, `Unreachable`, `TimedOut` and other connect errors |
| `lookup(host, port, &timeout, context)` | `Result[Resolving, NetError \| TimerError]` |
| `Resolving.handle(&mail, context)` | `Result[Option[list[SocketAddress]], NetError]`: `NameNotFound`, `TryAgain`, `TimedOut` |
| `Dialing.abandon(context)`, `Resolving.abandon(context)` | `Result[(), NetError]`: stop waiting and cancel the timer |

| Type | Values |
|---|---|
| `Admission` | `Pause`, `Refuse` |
| `Expired` | `Idle`, `Read`, `Write` (the stall deadline) |
| `Events` | As in [Connections](#connections); copyable |

## Limits

- TCP only; [`std.actor.tls`](actor-tls.md) adds TLS.
- Every child of the actor that owns a `Listener` counts as a connection;
  start other actors from another actor.
- The input and output buffers have fixed sizes. Input is read in place, so a
  message must fit in `Limits.input` to be handled whole.
- A trap closes the actor's sockets without a graceful shutdown; the peer
  sees a reset or end of input.
- Compacting a buffer copies its unconsumed bytes one at a time, since
  `Bytes` has no overlapping copy.
