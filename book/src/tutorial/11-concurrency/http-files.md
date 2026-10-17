# Serve files and sign cookies

[`std.http.files`](../../design/stdlib/http-files.md) answers a request
with a file from one directory, and
[`std.http.cookie`](../../design/stdlib/http-cookie.md) can sign a cookie
so the server can tell whether a client changed it.

```plenty
import std.bytes
import std.fs
import std.http
import std.http.cookie
import std.http.files
import std.http.server
import std.net
import std.path
import std.secrets
import std.time
from std.bytes import Bytes
from std.fs import Root
from std.http.server import Exchange
from std.net import TcpStream
from std.sync import CancellationToken

pub class Site:
    pub public: Root
    pub keys: list[Bytes]

    pub def handle(self, exchange: &mut Exchange[TcpStream]) -> Result[(), Failure]:
        if exchange.route == 0:
            path = exchange.params[0].concat("")?
            std.http.files.serve(exchange, &self.public, &path)?
        elif exchange.route == 1:
            mut head = std.http.response(204, "No Content")?
            cookie = std.http.cookie.signed_cookie(&self.keys, "user", "Ada")?
            std.http.cookie.add(&mut head, &cookie)?
            exchange.respond(head, &std.bytes.empty()?)?
        else:
            cookies = std.http.cookie.cookies(&exchange.request.headers, 32)?
            match std.http.cookie.find_signed(&cookies, &self.keys, "user")?:
                case Some(name):
                    exchange.send_text(200, &"hello ".concat(&name)?)?
                case Nothing:
                    exchange.send_text(401, "who are you?")?
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
    Ok(reply.to_text()?)

# The value of the field `name` in a response, or "".
def field(reply: &str, name: &str) -> Result[str, Failure]:
    for line in reply.split("\r\n")?:
        if line.startswith(name):
            return Ok(line.split(": ")?[1].concat("")?)
    Ok("")

def get(path: &str, extra: &str) -> Result[str, Failure]:
    Ok("GET ".concat(path)?.concat(" HTTP/1.1\r\nConnection: close\r\n")?.concat(extra)?.concat("\r\n")?)

def client(port: u16, stop: CancellationToken) -> Result[(), Failure]:
    page = fetch(port, &get("/files/index.html", "")?)?
    print(page.split("\r\n")?[0])?
    print(field(&page, "Content-Type")?)?
    print(page.split("\r\n\r\n")?[1].strip()?)?
    tag = field(&page, "ETag")?
    again = fetch(port, &get("/files/index.html", &"If-None-Match: ".concat(&tag)?.concat("\r\n")?)?)?
    print(again.split("\r\n")?[0])?
    for path in ["/files/../secret.txt", "/files/%2e%2e/secret.txt", "/files/.env"]?:
        print(fetch(port, &get(&path, "")?)?.split("\r\n")?[0])?

    login = fetch(port, "POST /login HTTP/1.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?
    cookie = field(&login, "Set-Cookie")?.split(";")?[0].concat("")?
    print(cookie.startswith("user=Ada."))?
    print(fetch(port, &get("/me", &"Cookie: ".concat(&cookie)?.concat("\r\n")?)?)?.split("\r\n\r\n")?[1])?
    forged = cookie.replace("Ada", "Eve")?
    print(fetch(port, &get("/me", &"Cookie: ".concat(&forged)?.concat("\r\n")?)?)?.split("\r\n\r\n")?[1])?
    stop.cancel()
    Ok(())

def main() -> Result[(), Failure]:
    parent = std.fs.root(&std.path.from_text(".")?)?
    mut directory = std.fs.temporary_dir(&parent, &std.path.from_text("plenty-site-")?, 8)?
    for name in ["index.html", ".env"]?:
        mut staged = std.fs.temporary_file(directory.root())?
        staged.stream.file.write("<h1>Welcome</h1>\n")?
        staged.publish(&std.path.from_text(name)?)?
        staged.close()?

    mut routes = std.http.server.router()?
    routes.add("GET", "/files/{path...}", 0)?
    routes.add("POST", "/login", 1)?
    routes.add("GET", "/me", 2)?
    site = Site(parent.open_dir(directory.name())?, [std.secrets.token_bytes(32)?]?)
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    with spawn(client, port, stop.share())? as task:
        std.http.server.serve(listener, &routes, &site, &std.http.server.config(), &stop)?
    for name in ["index.html", ".env"]?:
        directory.root().remove_file(&std.path.from_text(name)?)?
    directory.cleanup()?
    Ok(())
```
```output
HTTP/1.1 200 OK
text/html; charset=utf-8
<h1>Welcome</h1>
HTTP/1.1 304 Not Modified
HTTP/1.1 404 Not Found
HTTP/1.1 404 Not Found
HTTP/1.1 404 Not Found
True
hello Ada
who are you?
```

`serve` takes the path the route captured, already percent-decoded, and
opens it one segment at a time below `public`. A segment that is empty or
starts with `.` is refused, so `..` cannot climb out and hidden files such
as `.env` stay hidden, and a symbolic link is never followed, so a link
inside the directory cannot lead out of it either. Every refusal is a plain
`404`, which says nothing about what lies outside.

The type comes from the file's extension, and the response carries an
`ETag` and `Last-Modified` built from the file's size and modification
time. A browser that sends the tag back in `If-None-Match` gets `304 Not
Modified` with no body. The file itself is streamed in 16 KiB pieces, so a
large file does not need a large buffer.

`signed_cookie` appends a tag to the value: an HMAC of the cookie's name and
value under the first key, so `user=Ada.<tag>`. `find_signed` checks the
tag and gives back `Ada`; a client that changes the value to `Eve` cannot
compute a matching tag without the key, so the forged cookie is treated as
missing. The value is still readable by the client: signing stops changes,
not reading.

Keys are at least 32 random bytes, kept secret and stable across restarts;
`token_bytes` here makes a new one per run, which signs out every client
when the program restarts. To replace a key, put the new one first and keep
the old one after it until old cookies have expired: `find_signed` accepts
any listed key, and `signed_cookie` always signs with the first.
