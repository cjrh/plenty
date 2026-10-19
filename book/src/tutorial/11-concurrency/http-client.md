# Send HTTP requests

[`std.http.client`](../../design/stdlib/http-client.md) sends HTTP/1.1
requests and keeps finished connections open for the next request to the
same server. Every call takes a deadline and a cancellation token, so a slow
server cannot hold the caller longer than it allows.

This program runs a small [server](http-server.md) on a loopback port and
sends it three requests from another thread:

```plenty
import std.bytes
import std.http
import std.http.client
import std.http.server
import std.net
import std.time
from std.http.server import Exchange
from std.net import TcpStream
from std.sync import CancellationToken

pub enum Route:
    Home
    Old
    Echo

pub class Site:
    pub def handle(self, route: &Route, exchange: &mut Exchange[TcpStream]) -> Result[(), std.http.HttpError | std.bytes.BytesError]:
        match route:
            case Route.Home:
                exchange.send_text(200, "home\n")?
            case Route.Old:
                mut head = std.http.response(301, "Moved Permanently")?
                head.add("Location", "/")?
                exchange.respond(head, &std.bytes.empty()?)?
            case Route.Echo:
                mut body = std.bytes.empty()?
                exchange.read_all(&mut body)?
                exchange.send(200, "text/plain", &body)?
        Ok(())

def visit(port: u16, stop: CancellationToken) -> Result[(), AllocError | IoError | std.bytes.BytesError | std.http.client.ClientError | RangeError]:
    never = CancellationToken()?
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(5u32))?
    base = "http://127.0.0.1:".concat(&str.from(i64(port))?)?
    client = std.http.client.client(std.http.client.config()?)?

    home = client.get(&base.concat("/")?, &deadline, &never)?
    print(home.status, home.body.to_text()?.strip()?)?

    moved = client.get(&base.concat("/old")?, &deadline, &never)?
    print(moved.status, moved.redirects, moved.url.endswith("/"))?

    mut post = std.http.client.request("POST", &base.concat("/echo")?)?
    post.add("Content-Type", "text/plain")?
    post.body = std.bytes.from_text("ping")?
    echoed = client.fetch(post, &deadline, &never)?
    print(echoed.body.to_text()?)?

    stats = client.stats()
    print(stats.opened, stats.reused)?
    stop.cancel()
    Ok(())

def main() -> Result[(), ThreadError | std.http.server.ServerError | std.net.NetError | AllocError]:
    mut routes = std.http.server.router[Route]()?
    routes.add("GET", "/", Route.Home)?
    routes.add("GET", "/old", Route.Old)?
    routes.add("POST", "/echo", Route.Echo)?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    with spawn(visit, port, stop.share())? as task:
        drop(std.http.server.serve(listener, &routes, &Site(), &std.http.server.config(), &stop)?)
    Ok(())
```
```output
200 home
200 1 True
ping
1 3
```

`client(config())` makes a client for plain `http` URLs with the default
limits and timeouts. `get` and `fetch` send a request, read the whole
response body into `body`, and put the connection back in the client's
pool. All four requests here, counting the redirect, used one connection:
`opened` is 1 and `reused` is 3.

The client follows the `301` itself. `redirects` counts the hops and `url`
is the address that finally answered. When a redirect leaves the original
server, the client drops `Authorization` and `Cookie` so that credentials
meant for one server are not sent to another.

A request holds its method, URL, extra header fields and body. The client
writes `Host` and `Content-Length` itself and refuses requests that try to
set them.

`deadline` covers the whole exchange: connecting, sending, waiting for the
answer and reading the body. The config adds a limit for each phase, such
as `head_timeout` for the wait between sending a request and its response
starting. If either runs out, the call returns `Net(TimedOut)`.

A request that fails because a reused connection was closed by the server
is sent again on a new connection, but only for methods that are safe to
repeat, such as `GET` and `PUT`. A `POST` is never sent twice; its error is
returned so the program can decide what to do.
