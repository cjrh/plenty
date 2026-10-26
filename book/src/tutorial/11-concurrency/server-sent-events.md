# Stream events to browsers

Server-Sent Events keep one HTTP response open and send it a line-based
event each time something happens. Browsers read them with `EventSource`,
which reconnects by itself when the connection drops.
[`std.http.sse`](../../design/stdlib/sse.md) turns a
[server](http-server.md) exchange into an event stream.

This program serves `/clock` on a loopback port. The handler sends three
`tick` events a short interval apart, then an `end` event, and closes the
stream. The client, on another thread, reads the whole response with the
[HTTP client](http-client.md) and prints it without its final blank line.

```plenty
import std.bytes
import std.http
import std.http.client
import std.http.server
import std.http.sse
import std.net
import std.time
from std.fmt import FormatError
from std.http.server import Exchange
from std.http.sse import SseError
from std.net import TcpStream
from std.sync import CancellationToken
from std.time import Deadline

def after(milliseconds: u32) -> Result[Deadline, RangeError]:
    Ok(std.time.deadline_after(&std.time.monotonic_now(), &std.time.milliseconds(milliseconds))?)

pub enum Route:
    Clock

pub class Clock:
    pub def handle(self, route: &Route, exchange: &mut Exchange[TcpStream]) -> Result[(), SseError | FormatError | RangeError]:
        mut events = std.http.sse.accept(exchange)?
        events.retry(2000, &after(1000)?)?
        for n in range(3):
            match events.wait(&after(20)?):
                case Ok(_):
                    pass
                case Err(SseError.Stopping):
                    events.send("server stopping", &after(1000)?, event="end")?
                    return Ok(())
                case Err(error):
                    return Err(error)
            id = f"{n}"?
            events.send(&f"tick {n}\nall well"?, &after(1000)?, event="tick", id=Some(id))?
        events.send("done", &after(1000)?, event="end")?
        events.close()
        Ok(())

type WatchError = AllocError | IoError | FormatError | std.bytes.BytesError | std.http.client.ClientError | RangeError

def watch(port: u16, stop: CancellationToken) -> Result[(), WatchError]:
    with stop.cancel_on_exit():
        never = CancellationToken()?
        client = std.http.client.client(std.http.client.config()?)?
        reply = client.get(&f"http://127.0.0.1:{port}/clock"?, &after(5000)?, &never)?
        match std.http.find(&reply.head.headers, "content-type"):
            case Some(index):
                print(reply.status, reply.head.headers[index].value)?
            case Nothing:
                print(reply.status)?
        print(reply.body.to_text()?.strip()?)?
    Ok(())

def main() -> Result[(), ThreadError | std.http.server.ServerError | std.net.NetError | AllocError | WatchError]:
    mut routes = std.http.server.router[Route]()?
    routes.add("GET", "/clock", Route.Clock)?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    with spawn(watch, port, stop.share())? as task:
        drop(std.http.server.serve(listener, &routes, &Clock(), &std.http.server.config(), &stop)?)
        task.join()?
    Ok(())
```
```output
200 text/event-stream
retry: 2000

event: tick
id: 0
data: tick 0
data: all well

event: tick
id: 1
data: tick 1
data: all well

event: tick
id: 2
data: tick 2
data: all well

event: end
data: done
```

Each event is an optional `event` name, an optional `id`, and one `data`
line for each line of its text, ended by an empty line. A browser's
`EventSource` joins the `data` lines back together and calls the listener
for the event's name, or `message` when it has none. `retry` tells it how
long to wait before reconnecting. Every call writes and flushes whole
events before it returns, within its deadline.

The response has no length: it ends when the handler closes the stream, so
it is not cut off by the server's `response_timeout`. A client that
reconnects sends the last `id` it received, which the handler reads as
`events.last_event_id` to resume after it.

`wait` paces the stream. It returns `Ok` at its deadline, `Disconnected` as
soon as the client goes away, and `Stopping` when the server begins to shut
down; the stream is still open then, so the handler can say goodbye. A
`send` to a client that has gone away also fails with `Disconnected`. Each
open stream holds a server worker until its handler returns, so set
`config.workers` for the streams you expect.
