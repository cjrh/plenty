# Serve HTTP requests

[`std.http.server`](../../design/stdlib/http-server.md) runs an HTTP/1.1
server on a [`std.net`](tcp-sockets.md) listener. You give it a table of
routes and one handler; it accepts connections, frames requests with
[`std.http`](../08-resources/30-http-messages.md), and stops when a
cancellation token is cancelled.

```plenty
import std.bytes
import std.http.server
import std.net
import std.time
from std.http.server import Exchange
from std.net import TcpStream
from std.sync import CancellationToken

pub class Greeter:
    pub def handle(self, exchange: &mut Exchange[TcpStream]) -> Result[(), Failure]:
        if exchange.route == 0:
            exchange.send_text(200, "home\n")?
        else:
            greeting = "hello, ".concat(&exchange.params[0])?.concat("\n")?
            exchange.send_text(200, &greeting)?
        Ok(())

def get(port: u16, path: &str) -> Result[str, Failure]:
    never = CancellationToken()?
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(5u32))?
    mut stream = std.net.connect(&std.net.loopback_v4(port), &deadline, &never)?
    request = std.bytes.from_text(&"GET ".concat(path)?.concat(" HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?)?
    stream.write_all_before(&request, 0, request.len(), &deadline, &never)?
    mut reply = std.bytes.empty()?
    mut buffer = std.bytes.empty()?
    buffer.resize(512, 0u8)?
    while True:
        count = stream.read_before(&mut buffer, 0, 512, &deadline, &never)?
        if count == 0:
            break
        reply.extend_range(&buffer, 0, count)?
    text = reply.to_text()?
    status = text.split("\r\n")?[0].concat(" | ")?
    body = text.split("\r\n\r\n")?[1].strip()?
    Ok(status.concat(&body)?)

def client(port: u16, stop: CancellationToken) -> Result[(), Failure]:
    print(get(port, "/")?)?
    print(get(port, "/hello/Ada%20L")?)?
    print(get(port, "/missing")?)?
    stop.cancel()
    Ok(())

def main() -> Result[(), Failure]:
    mut routes = std.http.server.router()?
    routes.add("GET", "/", 0)?
    routes.add("GET", "/hello/{name}", 1)?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    with spawn(client, port, stop.share())? as task:
        report = std.http.server.serve(listener, &routes, &Greeter(), &std.http.server.config(), &stop)?
        print(report.requests)?
    Ok(())
```
```output
HTTP/1.1 200 OK | home
HTTP/1.1 200 OK | hello, Ada L
HTTP/1.1 404 Not Found | Not Found
3
```

`router()` matches the request path against each pattern in the order the
routes were added. `{name}` captures one path segment, and the handler finds
it, percent-decoded, in `exchange.params`. The number given to `add` comes
back as `exchange.route`, so the handler chooses what to do with a plain
`if`. A path with no route gets `404` without reaching the handler, and a
path whose route has another method gets `405` with an `Allow` field.

The client sends a `Host` field because HTTP/1.1 requires exactly one: a
request without it, with two, or with a malformed value gets `400` before
routing. The handler finds the host the request names in
`exchange.authority`.

The handler is a `pub class` with a `pub def handle`, because `serve`, in
another module, calls it. Every worker thread borrows the same handler, so
`handle` takes `self` by shared borrow and cannot change it.

`serve` runs on the calling thread until `stop` is cancelled. Here the client
thread cancels it after its last request. `serve` then stops accepting, lets
the connections it has finish within the drain timeout, joins its workers
and returns a `Report` of what it counted. `config()` gives the defaults:
four workers, a queue of sixteen waiting connections, and a deadline for
each phase of a request. A connection that arrives when the queue is full is
answered `503` at once instead of waiting.

A worker serves one request at a time, but it does not wait for a
keep-alive client's next request. Between requests the connection waits on
the server's own thread, on a [selector](tcp-sockets.md#wait-on-many-connections), and goes
back to the queue when the client sends more, so a few workers can keep many
idle clients open. `config.idle_connections` caps how many may wait.

## HTTPS

`std.http.tls.serve` is `serve` over [TLS](tls-streams.md). It takes the
`ServerConfig` from the TLS lesson, and the handler's exchanges carry a
`TlsTransport` instead of a `TcpStream`:

```text
import std.http.tls
from std.http.tls import TlsTransport

pub class Greeter:
    pub def handle(self, exchange: &mut Exchange[TlsTransport]) -> Result[(), Failure]:
        ...

server = std.tls.server_config(&leaf_pem(), &leaf_key()).unwrap()
report = std.http.tls.serve(listener, server, &routes, &Greeter(), &config, &stop)?
```

Each connection must finish its handshake within
`config.handshake_timeout` of being accepted, so a client that connects and
says nothing is closed then. Routing, deadlines and shutdown work as for
plain HTTP.
