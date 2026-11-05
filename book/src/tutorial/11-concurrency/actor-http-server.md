# Isolate HTTP connections with actors

[`std.actor.http.server`](../../design/stdlib/actor-http-server.md) gives each
connection its own handler and heap. A contained trap loses that connection;
other connections can continue. Its [qualification record](../../design/stdlib/actor-http-qualification.md)
states which deployment checks have passed.

Start with a complete, bounded request handler. This endpoint counts requests
on one connection. The ordinary HTTP client runs on a separate native thread;
blocking client operations must not run inside an actor callback.

```plenty
import std.actor.http.server
import std.bytes
import std.http
import std.http.client
import std.net
import std.time
from std.actor.http.server import Request, Response, ServerError
from std.sync import CancellationToken

type HandlerError = OverflowError[i64] | AllocError | std.http.HttpError | ServerError

pub class Counter:
    pub requests: i64

    pub def handle(self: &mut Counter, request: Request, response: &mut Response) -> Result[(), HandlerError]:
        if request.target.path != "/":
            response.send_text(404, "Not Found")?
            return Ok(())
        if request.head.method != "GET" and request.head.method != "HEAD":
            mut head = std.http.response(405, "Method Not Allowed")?
            head.add("Allow", "GET, HEAD")?
            response.respond(head, b"")?
            return Ok(())
        self.requests += 1
        response.send_text(200, &str.from(self.requests)?)?
        Ok(())

def make_counter() -> Result[Counter, HandlerError]:
    Ok(Counter(0))

type VisitError = AllocError | IoError | std.bytes.BytesError | std.http.client.ClientError | RangeError

def visit(port: u16, stop: CancellationToken) -> Result[(), VisitError]:
    with stop.cancel_on_exit():
        client = std.http.client.client(std.http.client.config()?)?
        url = f"http://127.0.0.1:{port}/"?
        deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(5u32))?
        never = CancellationToken()?
        for _ in range(2):
            reply = client.get(&url, &deadline, &never)?
            print(reply.status, reply.body.to_text()?)?
    Ok(())

def main() -> Result[(), SpawnError[()] | std.net.NetError | HandlerError | VisitError]:
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    with spawn(visit, port, stop.share()).drop_job()? as task:
        report = std.actor.http.server.serve(listener, make_counter, &std.actor.http.server.config(), &stop)?
        task.join()?
        print(report.requests)?
    Ok(())
```
```output
200 1
200 2
2
```

`make_counter` runs inside the new connection's heap. Two connections get
different counters; the client here reuses one connection. The mutable
`handle` receiver updates only that connection's state. Store longer-lived
text as owned `Bytes`; the request's `str` fields are available during the
callback but cannot become fields of the actor's handler.

The handler checks the method as well as the path, includes `Allow` in its
405 response, and propagates response errors. HEAD follows the GET handler
but does not send body bytes. The client cancels the server even if a request
fails, and `task.join()?` preserves that failure rather than printing success.

The complete request body is bounded by protocol limits and the actor heap.
A whole response must fit the configured output queue. Use the
[event interface](../../design/stdlib/actor-http-streaming.md) for larger
streams or multi-step nonblocking work: retain progress, handle short writes,
and resume when output becomes writable.

Use the [worker server](http-server.md) when sequential handlers and its
routing/application helpers suit the service. Use the actor server when
connection isolation justifies an event-driven application model. Neither
choice turns an infinite loop, stack overflow or foreign-library failure
into a safely recoverable request; consult the
[reliability boundary](../../design/stdlib/actor-http-server.md#reliability-boundary).
