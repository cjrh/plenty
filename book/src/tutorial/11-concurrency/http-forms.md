# Read forms, cookies and forwarded clients

A handler for [`std.http.server`](http-server.md) gets the raw request.
Three modules read the common parts of it safely:
[`std.http.body`](../../design/stdlib/http-body.md) reads forms, JSON and
uploads within limits, [`std.http.cookie`](../../design/stdlib/http-cookie.md)
reads and writes cookies, and
[`std.http.proxy`](../../design/stdlib/http-proxy.md) finds the client
behind a reverse proxy.

```plenty
import std.bytes
import std.http
import std.http.body
import std.http.cookie
import std.http.proxy
import std.http.server
import std.net
import std.time
from std.http.body import FormLimits
from std.http.proxy import Forwarding, Proxies
from std.http.server import Exchange
from std.net import TcpStream
from std.sync import CancellationToken

pub enum Route:
    Login
    Me

pub class Site:
    pub forms: FormLimits
    pub proxies: Proxies

    pub def handle(self, route: &Route, exchange: &mut Exchange[TcpStream]) -> Result[(), Failure]:
        match route:
            case Route.Login:
                match std.http.body.read_form(exchange, &self.forms):
                    case Ok(fields):
                        match std.http.body.find(&fields, "name"):
                            case Some(index):
                                name = &fields[index].value
                                mut head = std.http.response(200, "OK")?
                                std.http.cookie.add(&mut head, &std.http.cookie.set_cookie("user", name)?)?
                                head.add("Content-Type", "text/plain")?
                                mut text = "welcome ".concat(name)?
                                match std.http.body.find(&fields, "city"):
                                    case Some(city):
                                        text = text.concat(" from ")?.concat(&fields[city].value)?
                                    case Nothing:
                                        pass
                                exchange.respond(head, &std.bytes.from_text(&text)?)?
                            case Nothing:
                                exchange.send_text(400, "name is required")?
                    case Err(error):
                        std.http.body.reject(exchange, &error)?
            case Route.Me:
                cookies = std.http.cookie.cookies(&exchange.request.headers, 32)?
                client = std.http.proxy.client_of(exchange, &self.proxies)?
                mut text = std.net.format_ip(client.address)?.concat(" is ")?
                match std.http.cookie.find(&cookies, "user"):
                    case Some(index):
                        text = text.concat(&cookies[index].value)?
                    case Nothing:
                        text = text.concat("nobody")?
                exchange.send_text(200, &text)?
        Ok(())

def fetch(port: u16, request: &str) -> Result[str, Failure]:
    never = CancellationToken()?
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(5u32))?
    mut stream = std.net.connect(&std.net.loopback_v4(port), &deadline, &never)?
    data = std.bytes.from_text(request)?
    stream.write_all_before(&data, 0, data.len(), &deadline, &never)?
    mut reply = std.bytes.empty()?
    mut buffer = std.bytes.empty()?
    buffer.resize(512, 0u8)?
    while True:
        count = stream.read_before(&mut buffer, 0, 512, &deadline, &never)?
        if count == 0:
            break
        reply.extend_range(&buffer, 0, count)?
    text = reply.to_text()?
    mut summary = text.split("\r\n")?[0].concat("")?
    for line in text.split("\r\n")?:
        if line.startswith("Set-Cookie"):
            summary = summary.concat(" | ")?.concat(&line)?
    Ok(summary.concat(" | ")?.concat(&text.split("\r\n\r\n")?[1].strip()?)?)

def client(port: u16, stop: CancellationToken) -> Result[(), Failure]:
    form = "POST /login HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 28\r\nConnection: close\r\n\r\nname=Ada&city=S%C3%A3o+Paulo"
    print(fetch(port, form)?)?
    print(fetch(port, "POST /login HTTP/1.1\r\nHost: localhost\r\nContent-Type: text/plain\r\nContent-Length: 3\r\nConnection: close\r\n\r\nAda")?)?
    print(fetch(port, "GET /me HTTP/1.1\r\nHost: localhost\r\nCookie: theme=dark; user=Ada\r\nX-Forwarded-For: 198.51.100.4, 203.0.113.7\r\nConnection: close\r\n\r\n")?)?
    stop.cancel()
    Ok(())

def main() -> Result[(), Failure]:
    mut routes = std.http.server.router[Route]()?
    routes.add("POST", "/login", Route.Login)?
    routes.add("GET", "/me", Route.Me)?
    mut proxies = std.http.proxy.proxies(Forwarding.XForwardedFor)?
    proxies.trust("127.0.0.1")?
    site = Site(std.http.body.form_limits(), proxies)
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    with spawn(client, port, stop.share())? as task:
        std.http.server.serve(listener, &routes, &site, &std.http.server.config(), &stop)?
    Ok(())
```
```output
HTTP/1.1 200 OK | Set-Cookie: user=Ada; Path=/; Secure; HttpOnly; SameSite=Lax | welcome Ada from São Paulo
HTTP/1.1 415 Unsupported Media Type | Unsupported Media Type
HTTP/1.1 200 OK | 203.0.113.7 is Ada
```

`read_form` checks that the request says it carries a form, refuses a body
over `forms.max_bytes` (64 KiB by default) or with more than
`forms.max_fields` fields, and decodes each `name=value` pair: `+` is a
space and `%C3%A3` is the UTF-8 encoding of `ã`. It returns the pairs in order; `find` gives the
index of the first with a name. When it fails, `reject` answers with the
status that fits: `415` for the wrong type, `413` for too much, `400` for a
malformed body.

`set_cookie` checks the name and value and gives the cookie safe defaults:
`Secure`, so the browser sends it only over HTTPS, `HttpOnly`, so page
scripts cannot read it, and `SameSite=Lax`, so other sites cannot send it
with a form post. No value can add an attribute or another field: a name
with a space, such as `Ada L`, is an error rather than a broken header, so
encode free text before storing it in a cookie.

`cookies` reads every `Cookie` field. It skips pairs that are not valid
cookies instead of failing, because the field carries the cookies of every
application on the host.

The server sees its peer, `127.0.0.1`. When a reverse proxy forwards
requests, the peer is the proxy and the client's address is in
`X-Forwarded-For`, which anyone can write. `client_of` believes the header
only because the program trusts `127.0.0.1`, and reads it from the right:
`203.0.113.7` is the first address that is not a trusted proxy, so it is the
client. `198.51.100.4` came from the client itself and is ignored. With no
`trust` call, the header is ignored and the client is the peer.
