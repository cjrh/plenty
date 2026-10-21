# Serve HTTP requests

[`std.http.server`](../../design/stdlib/http-server.md) runs an HTTP/1.1
server on a [`std.net`](tcp-sockets.md) listener. You give it a table of
routes and one handler; it accepts connections, frames requests with
[`std.http`](../08-resources/30-http-messages.md), and stops when a
cancellation token is cancelled.

Each route maps a method and a path pattern to a value of your own route
type, normally an enum. The handler receives that value and matches on it.

```plenty
import std.http

import std.bytes
import std.http.server
import std.net
import std.time
from std.fmt import FormatError
from std.http.server import Exchange
from std.net import TcpStream
from std.sync import CancellationToken

pub enum Route:
    Home
    Hello(Greeting)

pub class Greeting:
    pub opening: str

    pub def handle(self, exchange: &mut Exchange[TcpStream]) -> Result[(), FormatError | std.http.HttpError]:
        text = f"{self.opening}{exchange.params[0]}\n"?
        exchange.send_text(200, &text)?
        Ok(())

pub class App:
    pub def handle(self, route: &Route, exchange: &mut Exchange[TcpStream]) -> Result[(), FormatError | std.http.HttpError]:
        match route:
            case Route.Home:
                exchange.send_text(200, "home\n")?
            case Route.Hello(greeting):
                greeting.handle(exchange)?
        Ok(())

type FetchError = AllocError | IoError | FormatError | std.bytes.BytesError | std.net.NetError | RangeError

def get(port: u16, path: &str) -> Result[str, FetchError]:
    never = CancellationToken()?
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(5u32))?
    mut stream = std.net.connect(&std.net.loopback_v4(port), &deadline, &never)?
    request = std.bytes.from_text(&f"GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"?)?
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
    status = text.split("\r\n")?[0]
    body = text.split("\r\n\r\n")?[1].strip()?
    Ok(f"{status} | {body}"?)

def client(port: u16, stop: CancellationToken) -> Result[(), FetchError]:
    with stop.cancel_on_exit():
        print(get(port, "/")?)?
        print(get(port, "/index.html")?)?
        print(get(port, "/hello/Ada%20L")?)?
        print(get(port, "/missing")?)?
    Ok(())

type ServerSetupError = std.http.server.ServerError | std.net.NetError

def main() -> Result[(), ThreadError | IoError | AllocError | ServerSetupError | FetchError]:
    mut routes = std.http.server.router[Route]()?
    routes.add("GET", "/", Route.Home)?
    routes.add("GET", "/index.html", Route.Home)?
    routes.add("GET", "/hello/{name}", Route.Hello(Greeting("hello, ")))?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    with spawn(client, port, stop.share())? as task:
        report = std.http.server.serve(listener, &routes, &App(), &std.http.server.config(), &stop)?
        task.join()?
        print(report.requests)?
    Ok(())
```
```output
HTTP/1.1 200 OK | home
HTTP/1.1 200 OK | home
HTTP/1.1 200 OK | hello, Ada L
HTTP/1.1 404 Not Found | Not Found
4
```

The client cancels `stop` when it is done, which stops the server. Its body
is in `with stop.cancel_on_exit():`, so a failed request stops the server too,
and `task.join()?` after `serve` reports that failure; see
[cancel on every exit](cooperative-cancellation.md#cancel-on-every-exit). The
later HTTP lessons use the same shape.

`FetchError` names the client's errors, shared by `get` and `client`.
`ServerSetupError` names this example's server and listener errors. `serve`
already wraps its network failures in `ServerError`; the separate `NetError`
member is needed because `main` opens and inspects the listener directly.
The alias composes with allocation, printing and thread-start errors without
mapping their values.

`router[Route]()` makes an empty table for the route type `Route`. It
matches the request path against each pattern in the order the routes were
added. `{name}` captures one path segment, and the handler finds it,
percent-decoded, in `exchange.params`. The value given to `add` comes back
to `handle` as `route`, borrowed from the table, so the handler chooses
what to do with a `match`. A route the handler forgets is a compile error:

```plenty-error
import std.http
from std.http.server import Exchange
from std.net import TcpStream

pub enum Route:
    Home
    Hello

pub class App:
    pub def handle(self, route: &Route, exchange: &mut Exchange[TcpStream]) -> Result[(), std.http.HttpError]:
        match route:
            case Route.Home:
                exchange.send_text(200, "home\n")?
        Ok(())

def main() -> Result[(), AllocError | IoError]:
    pass
    Ok(())
```

```error
non-exhaustive match; missing Route.Hello
```

A variant can carry the route's own handler and its state, as `Hello`
carries a `Greeting` here; `App` only forwards to it. Several patterns may
share a variant, such as `/` and `/index.html` both adding `Route.Home`.

A path with no route gets `404` without reaching the handler, and a path
whose route has another method gets `405` with an `Allow` field.

The client sends a `Host` field because HTTP/1.1 requires exactly one: a
request without it, with two, or with a malformed value gets `400` before
routing. The handler finds the host the request names in
`exchange.authority`.

The handler is a `pub class` with a `pub def handle`, because `serve`, in
another module, calls it, and `Route` is a `pub enum`, because `handle`'s
signature names it. Every worker thread borrows the same handler and the
same table, so `handle` takes `self` and `route` by shared borrow and
cannot change them.

`handle` returns its own error type, here `FormatError | std.http.HttpError`
from the f-string and `send_text`. The server answers an `Err` of any type with
`500` and counts it in `report.handler_errors`;
[When a handler fails](../../design/stdlib/http-server.md#when-a-handler-fails)
lists the other cases.

`serve` runs on the calling thread until `stop` is cancelled. Here the client
thread cancels it after its last request. `serve` then stops accepting, lets
the connections it has finish within the drain timeout, joins its workers
and returns a `Report` of what it counted. `config()` gives the defaults:
four workers, a queue of sixteen waiting connections, and a deadline for
each phase of a request. A connection that arrives when the queue is full is
answered `503` at once instead of waiting.

A worker serves one request at a time, but it does not wait for a client to
send one. Between requests, and while a request head is still arriving, the
connection waits on the server's own thread, on a
[selector](tcp-sockets.md#wait-on-many-connections), and goes back to the
queue once the client has sent a whole head. A few workers can therefore
keep many idle clients open, and clients that send slowly cannot hold them
all. `config.idle_connections` caps how many connections may wait.

## HTTPS

`std.http.tls.serve` is `serve` over [TLS](tls-streams.md). It takes the
`ServerConfig` from the TLS lesson, and the handler's exchanges carry a
`TlsTransport` instead of a `TcpStream`:

```text
import std.http.tls
from std.http.tls import TlsTransport

pub class App:
    pub def handle(self, route: &Route, exchange: &mut Exchange[TlsTransport]) -> Result[(), FormatError | std.http.HttpError]:
        ...

server = std.tls.server_config(&leaf_pem(), &leaf_key())?
report = std.http.tls.serve(listener, server, &routes, &App(), &config, &stop)?
```

Each connection must finish its handshake within
`config.handshake_timeout` of being accepted, so a client that connects and
says nothing is closed then. Between the client's handshake messages the
connection waits on the server's thread too. Routing, deadlines and shutdown work as for
plain HTTP.
