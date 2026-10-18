//! `std.http.body`, `std.http.cookie` and `std.http.proxy`: forms, JSON,
//! multipart uploads, cookies and forwarded clients. Requests run in memory
//! through `serve_connection_from`; nothing needs a network.

/// An in-memory `Transport` and a handler for each helper. `$STORAGE` is
/// replaced with a directory the test owns.
const PRELUDE: &str = r##"import std.bytes
import std.fs
import std.http
import std.http.body
import std.http.cookie
import std.http.proxy
import std.http.server
import std.io
import std.json
import std.net
import std.path
from std.bytes import ByteSource, Bytes
from std.fs import Root
from std.http.body import Content, FormLimits, MultipartLimits
from std.http.cookie import SameSite
from std.http.proxy import Forwarding, Proxies
from std.http.server import Config, Exchange, Router, Transport
from std.net import NetError, Shutdown, SocketAddress
from std.sync import CancellationToken
from std.time import Deadline

# Plays `input` seven bytes at a time and echoes what the server writes.
# When the input runs out it reports a clean end, or cancellation when `cut`
# is set.
pub class Script:
    pub input: Bytes
    pub at: i64
    pub cut: bool

    pub def read_before(self: &mut Script, buffer: &mut Bytes, start: i64, stop: i64, deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, NetError]:
        if cancel.is_cancelled():
            return Err(NetError.Cancelled)
        left = self.input.len() - self.at
        if left == 0:
            if self.cut:
                return Err(NetError.Cancelled)
            return Ok(0)
        mut count = stop - start
        if count > 7:
            count = 7
        if count > left:
            count = left
        buffer.overwrite(start, &self.input, self.at, self.at + count).unwrap()
        self.at += count
        Ok(count)

    pub def shutdown(self: &mut Script, direction: Shutdown) -> Result[(), NetError]:
        Ok(())

    pub def write_before[S: ByteSource](self: &mut Script, source: &S, start: i64, stop: i64, deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, NetError]:
        # Stdout is line buffered and may take only part of the range.
        Ok(source.write_stdout(start, stop).unwrap())

pub enum Route:
    Form
    Json
    Upload
    Cookies
    Client
    Search

pub class App:
    pub form: FormLimits
    pub multipart: MultipartLimits
    pub json: std.json.Limits
    pub storage: Root
    pub proxies: Proxies

    pub def handle(self, route: &Route, exchange: &mut Exchange[Script]) -> Result[(), Failure]:
        match route:
            case Route.Form:
                match std.http.body.read_form(exchange, &self.form):
                    case Ok(pairs):
                        mut text = ""
                        for pair in pairs:
                            text = text.concat("[")?.concat(&pair.key)?.concat("=")?.concat(&pair.value)?.concat("]")?
                        exchange.send_text(200, &text)?
                    case Err(error):
                        std.http.body.reject(exchange, &error)?
            case Route.Json:
                match std.http.body.read_json(exchange, &self.json):
                    case Ok(value):
                        std.http.body.send_json(exchange, 200, &value, &self.json)?
                    case Err(error):
                        std.http.body.reject(exchange, &error)?
            case Route.Upload:
                match std.http.body.read_multipart(exchange, &self.multipart, &self.storage):
                    case Ok(found):
                        print(open_files()?)?
                        mut parts = found
                        mut text = ""
                        for index in range(len(parts)):
                            part = &mut parts[index]
                            text = text.concat("[")?.concat(&part.name)?
                            match &part.filename:
                                case Some(name):
                                    text = text.concat(" file=")?.concat(name)?
                                case Nothing:
                                    pass
                            text = text.concat(" type=")?.concat(&part.content_type)?
                            text = text.concat(" size=")?.concat(str.from(part.size)?)?.concat(" ")?
                            size = part.size
                            match &mut part.content:
                                case Content.Text(value):
                                    text = text.concat(value)?
                                case Content.File(file):
                                    mut data = std.bytes.empty()?
                                    data.resize(size + 1, 0u8)?
                                    mut count = 0
                                    mut step = file.stream.read(&mut data, 0, size + 1)?
                                    while step > 0:
                                        count += step
                                        step = file.stream.read(&mut data, count, size + 1)?
                                    if count > 64:
                                        text = text.concat(data.slice(0, 8)?.to_text()?)?.concat("...")?
                                        text = text.concat(data.slice(count - 8, count)?.to_text()?)?
                                    else:
                                        text = text.concat(data.slice(0, count)?.to_text()?)?
                            text = text.concat("]")?
                        exchange.send_text(200, &text)?
                    case Err(error):
                        std.http.body.reject(exchange, &error)?
            case Route.Cookies:
                found = std.http.cookie.cookies(&exchange.request.headers, 4)?
                mut text = ""
                for cookie in found:
                    text = text.concat("[")?.concat(&cookie.name)?.concat("=")?.concat(&cookie.value)?.concat("]")?
                mut head = std.http.response(200, "OK")?
                mut session = std.http.cookie.set_cookie("session", "abc123")?
                session.max_age = Some(3600)
                std.http.cookie.add(&mut head, &session)?
                std.http.cookie.add(&mut head, &std.http.cookie.removal("old")?)?
                head.add("Content-Type", "text/plain")?
                body = std.bytes.from_text(&text)?
                exchange.respond(head, &body)?
            case Route.Client:
                match std.http.proxy.client_of(exchange, &self.proxies):
                    case Ok(client):
                        mut text = std.net.format_ip(client.address)?
                        match client.scheme:
                            case Some(scheme):
                                text = text.concat(" ")?.concat(&scheme)?
                            case Nothing:
                                pass
                        if client.proxied:
                            text = text.concat(" proxied")?
                        exchange.send_text(200, &text)?
                    case Err(_):
                        exchange.send_text(400, "bad forwarding")?
            case Route.Search:
                match std.http.body.parse_form_text(&exchange.query, &self.form):
                    case Ok(pairs):
                        match std.http.body.find(&pairs, "q"):
                            case Some(index):
                                exchange.send_text(200, &pairs[index].value)?
                            case Nothing:
                                exchange.send_text(200, "none")?
                    case Err(error):
                        std.http.body.reject(exchange, &error)?
        Ok(())

# Open file descriptors of this process.
pub def open_files() -> Result[str, Failure]:
    mut names = std.fs.root(&std.path.from_text("/proc/self/fd")?)?.names()?
    mut count = 0
    while True:
        match names.next()?:
            case Some(_):
                count += 1
            case Nothing:
                break
    Ok("fds=".concat(str.from(count)?)?)

pub def routes() -> Result[Router[Route], Failure]:
    mut table = std.http.server.router[Route]()?
    table.add("POST", "/form", Route.Form)?
    table.add("POST", "/json", Route.Json)?
    table.add("POST", "/upload", Route.Upload)?
    table.add("GET", "/cookies", Route.Cookies)?
    table.add("GET", "/client", Route.Client)?
    table.add("GET", "/search", Route.Search)?
    Ok(table)

pub def app() -> Result[App, Failure]:
    storage = std.fs.root(&std.path.from_text("$STORAGE")?)?
    Ok(App(std.http.body.form_limits(), std.http.body.multipart_limits(), std.json.limits(), storage, std.http.proxy.proxies(Forwarding.XForwardedFor)?))

# Serves `input` as one connection from `peer` and prints what was written.
pub def serve(app: &App, input: &str, peer: Option[SocketAddress]) -> Result[(), Failure]:
    play(app, input, peer, False)

pub def play(app: &App, input: &str, peer: Option[SocketAddress], cut: bool) -> Result[(), Failure]:
    table = routes()?
    stop = CancellationToken()?
    cancel = CancellationToken()?
    config = std.http.server.config()
    script = Script(std.bytes.from_text(input)?, 0, cut)
    report = std.http.server.serve_connection_from(script, peer, &table, app, &config, &stop, &cancel)
    print("")?
    Ok(())

pub def at(text: &str) -> Result[Option[SocketAddress], Failure]:
    Ok(Some(std.net.parse_address(text)?))
"##;

/// Compiles and runs `main` after the prelude. Response lines are joined by
/// `|`, and the Content-Type, Content-Length and Connection fields that every
/// response carries are dropped. Every test also checks that uploads left
/// nothing in the storage directory.
fn run(main: &str) -> String {
    let storage = tempfile::tempdir().unwrap();
    let source = format!(
        "{}{main}",
        PRELUDE.replace("$STORAGE", storage.path().to_str().unwrap())
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, &source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{main}\n{error}"));
    let output = std::process::Command::new(executable)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{main}\nstatus {:?}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_dir(storage.path()).unwrap().count(),
        0,
        "uploads left files behind"
    );
    let common = regex::Regex::new(
        r"(Content-Type: text/plain; charset=utf-8|Content-Type: application/json|Content-Length: \d+|Connection: close)\|",
    )
    .unwrap();
    let text = String::from_utf8_lossy(&output.stdout).replace("\r\n", "|");
    common
        .replace_all(&text, "")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn runs(main: &str, expected: &str) {
    let long_name = "n123456789".repeat(300);
    assert_eq!(run(main), expected.trim().replace("LONG", &long_name));
}

#[test]
fn forms_decode_and_reject_malformed_bodies() {
    runs(
        r##"
def form(body: &str) -> Result[str, Failure]:
    mut text = "POST /form HTTP/1.1\r\nHost: x\r\nContent-Type: application/x-www-form-urlencoded; charset=UTF-8\r\nConnection: close\r\nContent-Length: "
    text = text.concat(str.from(body.byte_len())?)?.concat("\r\n\r\n")?.concat(body)?
    Ok(text)

def main() -> Result[(), Failure]:
    mut app = app()?
    serve(&app, &form("a=1&b=two+words&&c&d=%C3%A9%26&e=%2B")?, Nothing)?
    serve(&app, &form("a=%zz")?, Nothing)?
    serve(&app, &form("a=%4")?, Nothing)?
    serve(&app, &form("a=%c3")?, Nothing)?
    serve(&app, "POST /form HTTP/1.1\r\nHost: x\r\nContent-Type: text/plain\r\nContent-Length: 3\r\nConnection: close\r\n\r\na=1", Nothing)?
    serve(&app, "POST /form HTTP/1.1\r\nHost: x\r\nContent-Length: 3\r\nConnection: close\r\n\r\na=1", Nothing)?
    serve(&app, "GET /search?q=hello+there&q=2 HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n", Nothing)?
    serve(&app, "GET /search?q=% HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n", Nothing)?
    Ok(())
"##,
        r"
HTTP/1.1 200 OK||[a=1][b=two words][c=][d=é&][e=+]
HTTP/1.1 400 Bad Request||Bad Request
HTTP/1.1 400 Bad Request||Bad Request
HTTP/1.1 400 Bad Request||Bad Request
HTTP/1.1 415 Unsupported Media Type||Unsupported Media Type
HTTP/1.1 415 Unsupported Media Type||Unsupported Media Type
HTTP/1.1 200 OK||hello there
HTTP/1.1 400 Bad Request||Bad Request
",
    );
}

/// The declared length is refused before the body is read; a chunked body
/// is refused once it passes the limit.
#[test]
fn form_limits_bound_bytes_and_fields() {
    runs(
        r##"
def main() -> Result[(), Failure]:
    mut app = app()?
    app.form.max_bytes = 4
    serve(&app, "POST /form HTTP/1.1\r\nHost: x\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 5\r\nConnection: close\r\n\r\na=123", Nothing)?
    serve(&app, "POST /form HTTP/1.1\r\nHost: x\r\nContent-Type: application/x-www-form-urlencoded\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\na=1\r\n3\r\n&b=\r\n0\r\n\r\n", Nothing)?
    app.form.max_bytes = 100
    app.form.max_fields = 2
    serve(&app, "POST /form HTTP/1.1\r\nHost: x\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 5\r\nConnection: close\r\n\r\na&b&c", Nothing)?
    serve(&app, "POST /form HTTP/1.1\r\nHost: x\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 6\r\nConnection: close\r\n\r\na&&&b&", Nothing)?
    app.form.max_fields = -1
    serve(&app, "POST /form HTTP/1.1\r\nHost: x\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 1\r\nConnection: close\r\n\r\na", Nothing)?
    Ok(())
"##,
        r"
HTTP/1.1 413 Content Too Large||Content Too Large
HTTP/1.1 413 Content Too Large||Content Too Large
HTTP/1.1 413 Content Too Large||Content Too Large
HTTP/1.1 200 OK||[a=][b=]
HTTP/1.1 500 Internal Server Error||Internal Server Error
",
    );
}

#[test]
fn json_bodies_are_typed_bounded_and_echoed() {
    runs(
        r##"
def main() -> Result[(), Failure]:
    mut app = app()?
    serve(&app, "POST /json HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: 14\r\nConnection: close\r\n\r\n{\"a\": [1, 2]} ", Nothing)?
    serve(&app, "POST /json HTTP/1.1\r\nHost: x\r\nContent-Type: application/problem+json; charset=utf-8\r\nContent-Length: 4\r\nConnection: close\r\n\r\ntrue", Nothing)?
    serve(&app, "POST /json HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: 5\r\nConnection: close\r\n\r\n{\"a\":", Nothing)?
    serve(&app, "POST /json HTTP/1.1\r\nHost: x\r\nContent-Type: text/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}", Nothing)?
    app.json.max_input = 3
    serve(&app, "POST /json HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: 4\r\nConnection: close\r\n\r\nnull", Nothing)?
    Ok(())
"##,
        r#"
HTTP/1.1 200 OK||{"a":[1,2]}
HTTP/1.1 200 OK||true
HTTP/1.1 400 Bad Request||Bad Request
HTTP/1.1 415 Unsupported Media Type||Unsupported Media Type
HTTP/1.1 413 Content Too Large||Content Too Large
"#,
    );
}

/// Multipart helpers: `upload(parts)` wraps already formatted parts in a
/// request with boundary `XyZ`.
const UPLOAD: &str = r##"
def upload(parts: &str) -> Result[str, Failure]:
    mut text = "POST /upload HTTP/1.1\r\nHost: x\r\nContent-Type: multipart/form-data; boundary=\"XyZ\"\r\nConnection: close\r\nContent-Length: "
    text = text.concat(str.from(parts.byte_len())?)?.concat("\r\n\r\n")?.concat(parts)?
    Ok(text)

def field(name: &str, value: &str) -> Result[str, Failure]:
    Ok("--XyZ\r\nContent-Disposition: form-data; name=\"".concat(name)?.concat("\"\r\n\r\n")?.concat(value)?.concat("\r\n")?)

def file(name: &str, value: &str) -> Result[str, Failure]:
    Ok("--XyZ\r\nContent-Disposition: form-data; name=\"".concat(name)?.concat("\"; filename=\"f.txt\"\r\nContent-Type: text/csv\r\n\r\n")?.concat(value)?.concat("\r\n")?)
"##;

/// Fields stay in memory and files go to unnamed temporary files that are
/// rewound for reading. The preamble, transport padding and epilogue are
/// skipped, and content that only resembles a delimiter is kept.
#[test]
fn multipart_fields_and_files_are_parsed() {
    runs(
        &format!(
            r##"{UPLOAD}
def main() -> Result[(), Failure]:
    mut app = app()?
    print(open_files()?)?
    serve(&app, "POST /upload HTTP/1.1\r\nHost: x\r\nContent-Type: multipart/form-data; boundary=\"XyZ\"\r\nContent-Length: 208\r\nConnection: close\r\n\r\npreamble\r\n--XyZ\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nhello\r\n--XyZ  \r\ncontent-disposition: form-data; name=\"doc\"; filename=\"a b.txt\"\r\nContent-Type: text/csv\r\n\r\nx,y\r\n--XyXnot\r\n1,2\r\n--XyZ--\r\nepilogue", Nothing)?
    print(open_files()?)?
    mut body = field("a", "")?
    body = body.concat(&file("b", "")?)?.concat("--XyZ--")?
    serve(&app, &upload(&body)?, Nothing)?
    serve(&app, &upload("--XyZ--")?, Nothing)?
    mut large = ""
    for index in range(5000):
        large = large.concat("abcdefg")?.concat(str.from(index % 10)?)?
    mut long_name = ""
    for index in range(300):
        long_name = long_name.concat("n123456789")?
    body = file("big", &large)?.concat(&field(&long_name, "v")?)?.concat(&file("again", &large)?)?.concat("--XyZ--")?
    serve(&app, &upload(&body)?, Nothing)?
    Ok(())
"##
        ),
        r"
fds=5
fds=7
HTTP/1.1 200 OK||[title type=text/plain size=5 hello][doc file=a b.txt type=text/csv size=18 x,y|--XyXnot|1,2]
fds=5
fds=7
HTTP/1.1 200 OK||[a type=text/plain size=0 ][b file=f.txt type=text/csv size=0 ]
fds=5
HTTP/1.1 200 OK||
fds=9
HTTP/1.1 200 OK||[big file=f.txt type=text/csv size=40000 abcdefg0...abcdefg9][LONG type=text/plain size=1 v][again file=f.txt type=text/csv size=40000 abcdefg0...abcdefg9]
",
    );
}

/// Each limit and each malformed shape is refused, and no temporary file
/// outlives the request.
#[test]
fn multipart_limits_and_malformed_bodies_leave_no_files() {
    runs(
        &format!(
            r##"{UPLOAD}
def main() -> Result[(), Failure]:
    mut app = app()?
    big = "0123456789abcdef"
    app.multipart.max_file = 15
    mut body = file("a", big)?
    serve(&app, &upload(&body.concat("--XyZ--")?)?, Nothing)?
    app.multipart.max_file = 16
    app.multipart.max_field = 15
    body = field("a", big)?
    serve(&app, &upload(&body.concat("--XyZ--")?)?, Nothing)?
    app.multipart.max_field = 16
    app.multipart.max_parts = 1
    body = field("a", "1")?.concat(&field("b", "2")?)?
    serve(&app, &upload(&body.concat("--XyZ--")?)?, Nothing)?
    app.multipart.max_parts = 8
    app.multipart.max_head = 40
    body = file("a", "1")?
    serve(&app, &upload(&body.concat("--XyZ--")?)?, Nothing)?
    app.multipart.max_head = 4096
    app.multipart.max_body = 60
    body = file("a", "1")?
    serve(&app, &upload(&body.concat("--XyZ--")?)?, Nothing)?
    app.multipart.max_body = 4096
    print(open_files()?)?
    serve(&app, &upload(&file("a", "unfinished")?)?, Nothing)?
    serve(&app, &upload("--XyZ\r\nContent-Disposition: attachment; name=\"a\"\r\n\r\nx\r\n--XyZ--")?, Nothing)?
    serve(&app, &upload("--XyZ\r\nContent-Type: text/plain\r\n\r\nx\r\n--XyZ--")?, Nothing)?
    serve(&app, &upload("--XyZ\r\nbroken header\r\n\r\nx\r\n--XyZ--")?, Nothing)?
    serve(&app, &upload("no delimiter at all")?, Nothing)?
    serve(&app, "POST /upload HTTP/1.1\r\nHost: x\r\nContent-Type: multipart/form-data\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", Nothing)?
    serve(&app, "POST /upload HTTP/1.1\r\nHost: x\r\nContent-Type: multipart/form-data; boundary=\"a@b\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", Nothing)?
    print(open_files()?)?
    Ok(())
"##
        ),
        r"
HTTP/1.1 413 Content Too Large||Content Too Large
HTTP/1.1 413 Content Too Large||Content Too Large
HTTP/1.1 413 Content Too Large||Content Too Large
HTTP/1.1 413 Content Too Large||Content Too Large
HTTP/1.1 413 Content Too Large||Content Too Large
fds=5
HTTP/1.1 400 Bad Request||Bad Request
HTTP/1.1 400 Bad Request||Bad Request
HTTP/1.1 400 Bad Request||Bad Request
HTTP/1.1 400 Bad Request||Bad Request
HTTP/1.1 400 Bad Request||Bad Request
HTTP/1.1 400 Bad Request||Bad Request
HTTP/1.1 400 Bad Request||Bad Request
fds=5
",
    );
}

/// A part header of exactly `max_head` bytes is accepted wherever the reads
/// split it. The script transport delivers 7 bytes per read, so the preamble
/// lengths cover every split, including a window that ends one byte before
/// the header terminator (issue #247).
#[test]
fn multipart_header_of_exactly_max_head_is_accepted_on_any_read_boundary() {
    runs(
        &format!(
            r##"{UPLOAD}
def main() -> Result[(), Failure]:
    mut app = app()?
    # `Content-Disposition: form-data; name="a"` is 40 bytes.
    app.multipart.max_head = 40
    mut preamble = ""
    for index in range(7):
        body = preamble.concat("\r\n")?.concat(&field("a", "1")?)?.concat("--XyZ--")?
        serve(&app, &upload(&body)?, Nothing)?
        preamble = preamble.concat("p")?
    app.multipart.max_head = 39
    serve(&app, &upload(&field("a", "1")?.concat("--XyZ--")?)?, Nothing)?
    Ok(())
"##
        ),
        r"
fds=5
HTTP/1.1 200 OK||[a type=text/plain size=1 1]
fds=5
HTTP/1.1 200 OK||[a type=text/plain size=1 1]
fds=5
HTTP/1.1 200 OK||[a type=text/plain size=1 1]
fds=5
HTTP/1.1 200 OK||[a type=text/plain size=1 1]
fds=5
HTTP/1.1 200 OK||[a type=text/plain size=1 1]
fds=5
HTTP/1.1 200 OK||[a type=text/plain size=1 1]
fds=5
HTTP/1.1 200 OK||[a type=text/plain size=1 1]
HTTP/1.1 413 Content Too Large||Content Too Large
",
    );
}

/// A connection cancelled mid-upload is answered 408, and the temporary file
/// it was writing is closed.
#[test]
fn a_cancelled_upload_releases_its_file() {
    runs(
        &format!(
            r##"{UPLOAD}
def main() -> Result[(), Failure]:
    mut app = app()?
    print(open_files()?)?
    mut text = "POST /upload HTTP/1.1\r\nHost: x\r\nContent-Type: multipart/form-data; boundary=XyZ\r\nContent-Length: 1000\r\n\r\n"
    text = text.concat(&file("a", "partial content")?)?
    play(&app, &text, Nothing, True)?
    print(open_files()?)?
    Ok(())
"##
        ),
        r"
fds=5
HTTP/1.1 408 Request Timeout||Request Timeout
fds=5
",
    );
}

#[test]
fn cookies_are_read_leniently_and_written_strictly() {
    runs(
        r##"
def main() -> Result[(), Failure]:
    mut app = app()?
    serve(&app, "GET /cookies HTTP/1.1\r\nHost: x\r\nCookie: a=1; b=\"two\"; bad name=3; c=x,y; =4; d\r\nCookie: e=\r\nConnection: close\r\n\r\n", Nothing)?
    serve(&app, "GET /cookies HTTP/1.1\r\nHost: x\r\nCookie: a=1; b=2; c=3; d=4; e=5\r\nConnection: close\r\n\r\n", Nothing)?
    mut evil = std.http.cookie.set_cookie("id", "x")?
    evil.value = "x\r\nSet-Cookie: admin=1"
    print(evil.format().is_err())?
    evil.value = "x; Domain=evil.example"
    print(evil.format().is_err())?
    evil.value = "x"
    evil.name = "id\r\nX: y"
    print(evil.format().is_err())?
    evil.name = "id"
    evil.path = "/\r\nX: y"
    print(evil.format().is_err())?
    evil.path = "/a;b"
    print(evil.format().is_err())?
    evil.path = "/"
    evil.domain = "evil.example; Secure"
    print(evil.format().is_err())?
    print(std.http.cookie.set_cookie("a b", "x").is_err())?
    print(std.http.cookie.set_cookie("a", "\"x\"").is_err())?
    mut host = std.http.cookie.set_cookie("__Host-id", "x")?
    print(host.format()?)?
    host.domain = "example.com"
    print(host.format().is_err())?
    mut secure = std.http.cookie.set_cookie("__Secure-id", "x")?
    secure.secure = False
    print(secure.format().is_err())?
    mut cross = std.http.cookie.set_cookie("x", "y")?
    cross.same_site = SameSite.Unrestricted
    print(cross.format()?)?
    cross.secure = False
    print(cross.format().is_err())?
    cross.same_site = SameSite.Strict
    cross.path = ""
    cross.domain = "a.example"
    cross.max_age = Some(-1)
    print(cross.format().is_err())?
    cross.max_age = Nothing
    cross.http_only = False
    print(cross.format()?)?
    Ok(())
"##,
        r"
HTTP/1.1 200 OK|Set-Cookie: session=abc123; Path=/; Max-Age=3600; Secure; HttpOnly; SameSite=Lax|Set-Cookie: old=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Lax|Content-Type: text/plain||[a=1][b=two][e=]
HTTP/1.1 500 Internal Server Error||Internal Server Error
True
True
True
True
True
True
True
True
__Host-id=x; Path=/; Secure; HttpOnly; SameSite=Lax
True
True
x=y; Path=/; Secure; HttpOnly; SameSite=None
True
True
x=y; Domain=a.example; SameSite=Strict
",
    );
}

/// Forwarding headers count only from trusted proxies, read right to left;
/// whatever the client wrote left of the first untrusted hop is not believed.
/// Issue #285: `X-Forwarded-For` entries there are not parsed, but every
/// `Forwarded` element is, so a malformed one fails wherever it is.
#[test]
fn forwarded_clients_need_trusted_proxies() {
    runs(
        r##"
def main() -> Result[(), Failure]:
    mut app = app()?
    get = "GET /client HTTP/1.1\r\nHost: x\r\nX-Forwarded-For: 6.6.6.6, 203.0.113.9, 10.0.0.7\r\nX-Forwarded-Proto: https\r\nForwarded: for=7.7.7.7\r\nConnection: close\r\n\r\n"
    serve(&app, get, Nothing)?
    serve(&app, get, at("198.51.100.1:5000")?)?
    app.proxies.trust("10.0.0.0/8")?
    serve(&app, get, at("198.51.100.1:5000")?)?
    serve(&app, get, at("10.1.2.3:5000")?)?
    serve(&app, get, at("[::ffff:10.1.2.3]:5000")?)?
    app.proxies.trust("203.0.113.0/24")?
    serve(&app, get, at("10.1.2.3:5000")?)?
    serve(&app, "GET /client HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n", at("10.1.2.3:5000")?)?
    serve(&app, "GET /client HTTP/1.1\r\nHost: x\r\nX-Forwarded-For: garbage, 10.0.0.9\r\nConnection: close\r\n\r\n", at("10.1.2.3:5000")?)?
    serve(&app, "GET /client HTTP/1.1\r\nHost: x\r\nX-Forwarded-For: 10.0.0.1:443, [2001:db8::1]:80\r\nConnection: close\r\n\r\n", at("10.1.2.3:5000")?)?
    serve(&app, "GET /client HTTP/1.1\r\nHost: x\r\nX-Forwarded-For: 10.0.0.1\r\nX-Forwarded-For: 10.0.0.2\r\nConnection: close\r\n\r\n", at("10.1.2.3:5000")?)?
    serve(&app, "GET /client HTTP/1.1\r\nHost: x\r\nX-Forwarded-For: unknown\r\nConnection: close\r\n\r\n", at("10.1.2.3:5000")?)?
    serve(&app, "GET /client HTTP/1.1\r\nHost: x\r\nX-Forwarded-For: 1.1.1.1\r\nX-Forwarded-Proto: gopher\r\nConnection: close\r\n\r\n", at("10.1.2.3:5000")?)?
    serve(&app, "GET /client HTTP/1.1\r\nHost: x\r\nX-Forwarded-For: garbage, 192.0.2.1\r\nConnection: close\r\n\r\n", at("10.1.2.3:5000")?)?
    app.proxies.max_hops = 2
    serve(&app, get, at("10.1.2.3:5000")?)?
    app.proxies.header = Forwarding.Forwarded
    serve(&app, "GET /client HTTP/1.1\r\nHost: x\r\nX-Forwarded-For: 1.1.1.1\r\nForwarded: for=\"[2001:db8::7]:99\";proto=https, For=192.0.2.60;proto=http;by=10.0.0.1\r\nConnection: close\r\n\r\n", at("10.1.2.3:5000")?)?
    serve(&app, "GET /client HTTP/1.1\r\nHost: x\r\nForwarded: for=\"[2001:db8::7]:99\";proto=https, for=10.0.0.3\r\nConnection: close\r\n\r\n", at("10.1.2.3:5000")?)?
    serve(&app, "GET /client HTTP/1.1\r\nHost: x\r\nForwarded: for=_hidden, for=10.0.0.3\r\nConnection: close\r\n\r\n", at("10.1.2.3:5000")?)?
    serve(&app, "GET /client HTTP/1.1\r\nHost: x\r\nForwarded: garbage, for=192.0.2.1\r\nConnection: close\r\n\r\n", at("10.1.2.3:5000")?)?
    serve(&app, "GET /client HTTP/1.1\r\nHost: x\r\nForwarded: for=\"a,b\r\nConnection: close\r\n\r\n", at("10.1.2.3:5000")?)?
    print(app.proxies.trust("10.0.0.1/8").is_err())?
    print(app.proxies.trust("10.0.0.0/33").is_err())?
    print(app.proxies.trust("10.0.0.0/").is_err())?
    print(app.proxies.trust("localhost").is_err())?
    print(app.proxies.trust("fd00::1/8").is_err())?
    print(app.proxies.trust("fd00::/8").is_ok())?
    print(app.proxies.trust("0.0.0.0/0").is_ok())?
    print(app.proxies.trusts(&std.net.parse_ip("fd12::1")?))?
    print(app.proxies.trusts(&std.net.parse_ip("fe00::1")?))?
    Ok(())
"##,
        r"
HTTP/1.1 400 Bad Request||bad forwarding
HTTP/1.1 200 OK||198.51.100.1
HTTP/1.1 200 OK||198.51.100.1
HTTP/1.1 200 OK||203.0.113.9 https proxied
HTTP/1.1 200 OK||203.0.113.9 https proxied
HTTP/1.1 200 OK||6.6.6.6 https proxied
HTTP/1.1 200 OK||10.1.2.3
HTTP/1.1 400 Bad Request||bad forwarding
HTTP/1.1 200 OK||2001:db8::1 proxied
HTTP/1.1 200 OK||10.0.0.1 proxied
HTTP/1.1 400 Bad Request||bad forwarding
HTTP/1.1 400 Bad Request||bad forwarding
HTTP/1.1 200 OK||192.0.2.1 proxied
HTTP/1.1 400 Bad Request||bad forwarding
HTTP/1.1 200 OK||192.0.2.60 http proxied
HTTP/1.1 200 OK||2001:db8::7 https proxied
HTTP/1.1 400 Bad Request||bad forwarding
HTTP/1.1 400 Bad Request||bad forwarding
HTTP/1.1 400 Bad Request||bad forwarding
True
True
True
True
True
True
True
True
False
",
    );
}

/// Issue #135: `set_cookie` shares its name and value instead of copying
/// them, so it allocates nothing, and the cookie outlives its arguments.
#[cfg(feature = "runtime-checks")]
#[test]
fn set_cookie_shares_its_arguments() {
    runs(
        r##"
def made() -> Result[std.http.cookie.SetCookie, Failure]:
    name = "a-cookie-name".concat("-too-long-for-inline-storage")?
    value = "a-cookie-value".concat("-too-long-for-inline-storage")?
    print("__test_begin_no_allocations__")?
    cookie = std.http.cookie.set_cookie(&name, &value)
    print("__test_end_no_allocations__")?
    Ok(cookie?)

def main() -> Result[(), Failure]:
    cookie = made()?
    print(cookie.name, cookie.value)?
    print(std.http.cookie.set_cookie("", "x").is_err())?
    Ok(())
"##,
        "a-cookie-name-too-long-for-inline-storage a-cookie-value-too-long-for-inline-storage\nTrue",
    );
}
