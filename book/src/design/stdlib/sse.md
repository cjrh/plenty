# `std.http.sse`

`import std.http.sse` sends
[Server-Sent Events](https://html.spec.whatwg.org/multipage/server-sent-events.html),
the `text/event-stream` format browsers read with `EventSource`, from a
[`std.http.server`](http-server.md) handler. The handler turns its exchange
into an event stream that outlives the server's response deadline, then
sends events until it closes the stream, the client goes away, or the
server shuts down. The module is checked Plenty in
`stdlib/std/http/sse.plenty` and adds no runtime primitives. The tutorial
lesson [Stream events to browsers](../../tutorial/11-concurrency/server-sent-events.md)
shows a complete program. There is no client-side reader.

```plenty
import std.bytes
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

pub enum Route:
    Events

pub class Feed:
    pub def handle(self, route: &Route, exchange: &mut Exchange[TcpStream]) -> Result[(), SseError | RangeError]:
        deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(1u32))?
        mut events = std.http.sse.accept(exchange)?
        events.send("first line\nsecond line", &deadline, event="note", id=Some("1"))?
        events.send("done", &deadline)?
        events.close()
        Ok(())

type WatchError = AllocError | IoError | FormatError | std.bytes.BytesError | std.http.client.ClientError | RangeError

def watch(port: u16, stop: CancellationToken) -> Result[(), WatchError]:
    with stop.cancel_on_exit():
        never = CancellationToken()?
        deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(5u32))?
        client = std.http.client.client(std.http.client.config()?)?
        reply = client.get(&f"http://127.0.0.1:{port}/events"?, &deadline, &never)?
        print(reply.body.to_text()?.strip()?)?
    Ok(())

def main() -> Result[(), ThreadError | std.http.server.ServerError | std.net.NetError | WatchError]:
    mut routes = std.http.server.router[Route]()?
    routes.add("GET", "/events", Route.Events)?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    with spawn(watch, port, stop.share())? as task:
        drop(std.http.server.serve(listener, &routes, &Feed(), &std.http.server.config(), &stop)?)
        task.join()?
    Ok(())
```
```output
event: note
id: 1
data: first line
data: second line

data: done
```

## Opening a stream

| Item | Meaning |
|---|---|
| `accept(exchange: &mut Exchange[T])` | `Result[EventStream[T], SseError]`: answer with `head()` and return the stream |
| `accept_with(exchange, head: ResponseHead)` | `accept` with the caller's `200` head, such as one with CORS fields |
| `head()` | `Result[ResponseHead, HttpError]`: the head `accept` sends |

The response is:

```text
HTTP/1.1 200 OK
Content-Type: text/event-stream
Cache-Control: no-cache
Connection: close
```

`accept_with` adds `Content-Type` and `Cache-Control` unless the head names
them, and refuses a status other than `200`
(`Http(InvalidStatusLine)`) and a head that declares `Content-Length` or
`Transfer-Encoding` (`Http(ConflictingFraming)`). A refused head leaves the
exchange unanswered, so the handler can still answer it. The request body
must be empty or read to its end (`Http(Unfinished)`).

The body is delimited by the connection's close rather than chunked: the
connection leaves the server through
[`Exchange.detach`](http-server.md#detaching-a-connection), so the server's
`response_timeout` no longer applies, and dropping or closing the stream
closes the connection. A `HEAD` request is answered with the head alone,
with the server's usual framing, and `accept` returns `Closed`.

`stream.last_event_id` is the request's `Last-Event-ID` field, which a
reconnecting `EventSource` sends with the last id it received, or
`Nothing`. A handler that keeps a history can resume after it.

## Sending

| Method | Meaning |
|---|---|
| `send(&data, &deadline, event="", id=Nothing)` | Send one event |
| `send_bytes(&data, start, stop, &deadline, event="", id=Nothing)` | `send` of UTF-8 `data[start:stop]`, without copying it first |
| `comment(&text, &deadline)` | Send comment lines, which clients ignore |
| `retry(milliseconds, &deadline)` | Ask the client to wait this long before reconnecting |
| `wait(&deadline)` | Wait until `deadline`, watching for a disconnect and for shutdown |
| `close()` | End the response; closing a closed stream does nothing |
| `is_open()` | Whether events can still be sent |

These return `Result[(), SseError]`, except `close` and `is_open`. Each
sending call writes and flushes whole lines before it returns, within its
deadline.

An event is written as the format's readers parse it:

| Part | Lines |
|---|---|
| `event` | `event: name`, unless empty; the client's default type is `message` |
| `id` | `id: value` when given; `Some("")` resets the client's last event id |
| `data` | `data: line` for each line; CRLF, CR and LF all end a line |
| end | an empty line, which dispatches the event |

`send("a\nb", &deadline, event="update", id=Some("7"))` writes
`event: update\nid: 7\ndata: a\ndata: b\n\n`. Data ending with a line break
gets a last, empty `data:` line, so the client sees the trailing line feed.
An event name or id holding CR or LF, or an id holding NUL, is
`InvalidField`; data that is not UTF-8 is `InvalidText`; a range out of
bounds or a negative retry time is `OutOfRange`. These checks happen before
anything is written, and the stream stays open.

`comment("ping", &deadline)` writes `: ping\n`, one line per line of text.
A comment is the usual keep-alive: proxies close quiet connections, and a
write is how a server notices a client that has gone away.

## Disconnects and shutdown

A client that closes or resets the connection makes the next write fail
with `Disconnected`. `wait` reads from the connection while it waits, so it
returns `Disconnected` as soon as the client closes, without a write.
Anything the client sends is discarded. A write that times out, or fails
part-way through an event, also closes the stream, since the client is left
inside an event. Every connection error closes the stream; later calls fail
with `Closed`. `Stopping`, the field checks above and allocation failures
leave it open.

The stream keeps the server's shutdown tokens. `wait` ends with `Stopping`
when shutdown begins, and the stream stays open so the handler can send a
last event; a later `wait` returns `Stopping` at once. When the drain
deadline passes, writes and `wait` end with `Net(Cancelled)`.

A handler sends events between waits:

```text
while True:
    events.send(&next_update()?, &soon()?)?
    match events.wait(&in_one_second()?):
        case Ok(_):
            pass
        case Err(SseError.Stopping):
            events.send("bye", &soon()?, event="shutdown")?
            return Ok(())
        case Err(error):
            return Err(error)
```

## Errors

`SseError` has these cases:

| Case | Meaning |
|---|---|
| `Net(NetError)` | The connection failed, timed out, or was cancelled |
| `Allocation(AllocError)` | A buffer could not grow |
| `Http(HttpError)` | The head was refused or could not be written |
| `InvalidField` | An event name or id with a line break, or an id with NUL |
| `InvalidText` | Data that is not UTF-8 |
| `OutOfRange` | A byte range out of bounds, or a negative retry time |
| `Disconnected` | The client closed the connection |
| `Closed` | The stream was closed, or failed earlier |
| `Stopping` | Server shutdown began while waiting |

## Limits

- An open stream holds its server worker until the handler returns, as a
  [WebSocket](websocket.md) does, so each one counts against `workers`.
  Route event streams to handlers that return when the stream ends.
- One caller at a time: the methods borrow the stream exclusively, so one
  thread cannot wait while another sends. A handler that needs events from
  other threads receives them through a [channel](sync.md) between waits.
- No event history: resuming after `last_event_id` is the handler's job.
- No client-side reader.
- A stream holds a 256-byte output buffer, grown to the largest event sent,
  and a 512-byte buffer for discarded input.
