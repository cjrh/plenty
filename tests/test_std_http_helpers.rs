//! `std.http.body`, `std.http.cookie` and `std.http.proxy`: forms, JSON,
//! multipart uploads, cookies and forwarded clients. Requests run in memory
//! through `serve_connection_from`; nothing needs a network.

/// An in-memory `Transport` and a handler for each helper. `$STORAGE` is
/// replaced with a directory the test owns.
const PRELUDE: &str = r##"import std.bytes
import std.time
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
from std.bytes import Bytes
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
        # Validated transport ranges and 0 <= at <= len(input); count is
        # bounded by both remaining ranges, so advancing stays within input.
        unchecked:
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
            buffer.overwrite(start, &self.input[self.at:self.at + count])
            self.at += count
            Ok(count)

    pub def shutdown(self: &mut Script, direction: Shutdown) -> Result[(), NetError]:
        Ok(())

    pub def write_before(self: &mut Script, source: &[u8], deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, NetError]:
        # Stdout is line buffered and may take only part of the range.
        mut out = std.io.stdout()
        Ok(out.write(source).unwrap())

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
    pub baseline_files: i64

    pub def handle(self, route: &Route, exchange: &mut Exchange[Script]) -> Result[(), std.http.body.BodyError | std.bytes.BytesError | std.http.cookie.CookieError | std.http.HttpError | std.net.NetError | std.io.StreamError | AllocError | IoError | OverflowError[i64]]:
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
                        print(file_delta(self.baseline_files)?)?
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
                session.max_age = Some(std.time.seconds(3600u32))
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
pub def open_files() -> Result[i64, IoError | OverflowError[i64]]:
    mut names = std.fs.root(&std.path.from_text("/proc/self/fd")?)?.names()?
    mut count = 0
    while True:
        match names.next()?:
            case Some(_):
                count += 1
            case Nothing:
                break
    Ok(count)

pub def file_delta(baseline: i64) -> Result[str, AllocError | IoError | OverflowError[i64]]:
    Ok("fds=".concat(str.from(open_files()? - baseline)?)?)

pub def routes() -> Result[Router[Route], std.http.server.ServerError]:
    mut table = std.http.server.router[Route]()?
    table.add("POST", "/form", Route.Form)?
    table.add("POST", "/json", Route.Json)?
    table.add("POST", "/upload", Route.Upload)?
    table.add("GET", "/cookies", Route.Cookies)?
    table.add("GET", "/client", Route.Client)?
    table.add("GET", "/search", Route.Search)?
    Ok(table)

pub def app() -> Result[App, std.http.proxy.ProxyError | IoError | OverflowError[i64]]:
    storage = std.fs.root(&std.path.from_text("$STORAGE")?)?
    # Keep the storage root live for both the baseline and request measurements.
    baseline = open_files()?
    Ok(App(std.http.body.form_limits(), std.http.body.multipart_limits(), std.json.limits(), storage, std.http.proxy.proxies(Forwarding.XForwardedFor)?, baseline))

# Serves `input` as one connection from `peer` and prints what was written.
pub def serve(app: &App, input: &str, peer: Option[SocketAddress]) -> Result[(), std.bytes.BytesError | std.http.server.ServerError | AllocError | IoError | OverflowError[i64]]:
    play(app, input, peer, False)

pub def play(app: &App, input: &str, peer: Option[SocketAddress], cut: bool) -> Result[(), std.bytes.BytesError | std.http.server.ServerError | AllocError | IoError | OverflowError[i64]]:
    table = routes()?
    stop = CancellationToken()?
    cancel = CancellationToken()?
    config = std.http.server.config()
    script = Script(std.bytes.from_text(input)?, 0, cut)
    report = std.http.server.serve_connection_from(script, peer, &table, app, &config, &stop, &cancel)
    if open_files()? != app.baseline_files:
        print("request leaked descriptors", file_delta(app.baseline_files)?)?
    print("")?
    Ok(())

pub def at(text: &str) -> Result[Option[SocketAddress], std.net.NetError]:
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
def form(body: &str) -> Result[str, AllocError]:
    mut text = "POST /form HTTP/1.1\r\nHost: x\r\nContent-Type: application/x-www-form-urlencoded; charset=UTF-8\r\nConnection: close\r\nContent-Length: "
    text = text.concat(str.from(body.byte_len())?)?.concat("\r\n\r\n")?.concat(body)?
    Ok(text)

def main() -> Result[(), std.bytes.BytesError | std.http.proxy.ProxyError | std.http.server.ServerError | AllocError | IoError | OverflowError[i64]]:
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
def main() -> Result[(), std.bytes.BytesError | std.http.proxy.ProxyError | std.http.server.ServerError | AllocError | IoError | OverflowError[i64]]:
    mut app = app()?
    app.form.max_bytes = 4
    serve(&app, "POST /form HTTP/1.1\r\nHost: x\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 5\r\nConnection: close\r\n\r\na=123", Nothing)?
    serve(&app, "POST /form HTTP/1.1\r\nHost: x\r\nContent-Type: application/x-www-form-urlencoded\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\na=1\r\n3\r\n&b=\r\n0\r\n\r\n", Nothing)?
    app.form.max_bytes = 100
    app.form.max_fields = 2
    serve(&app, "POST /form HTTP/1.1\r\nHost: x\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 5\r\nConnection: close\r\n\r\na&b&c", Nothing)?
    serve(&app, "POST /form HTTP/1.1\r\nHost: x\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 6\r\nConnection: close\r\n\r\na&&&b&", Nothing)?
    Ok(())
"##,
        r"
HTTP/1.1 413 Content Too Large||Content Too Large
HTTP/1.1 413 Content Too Large||Content Too Large
HTTP/1.1 413 Content Too Large||Content Too Large
HTTP/1.1 200 OK||[a=][b=]
",
    );
}

#[test]
fn json_bodies_are_typed_bounded_and_echoed() {
    runs(
        r##"
def main() -> Result[(), std.bytes.BytesError | std.http.proxy.ProxyError | std.http.server.ServerError | AllocError | IoError | OverflowError[i64]]:
    mut app = app()?
    serve(&app, "POST /json HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: 14\r\nConnection: close\r\n\r\n{\"a\": [1, 2]} ", Nothing)?
    serve(&app, "POST /json HTTP/1.1\r\nHost: x\r\nContent-Type: application/problem+json; charset=utf-8\r\nContent-Length: 4\r\nConnection: close\r\n\r\ntrue", Nothing)?
    serve(&app, "POST /json HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: 5\r\nConnection: close\r\n\r\n{\"a\":", Nothing)?
    serve(&app, "POST /json HTTP/1.1\r\nHost: x\r\nContent-Type: text/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}", Nothing)?
    # A second '/' makes the media type malformed, though it ends in +json.
    serve(&app, "POST /json HTTP/1.1\r\nHost: x\r\nContent-Type: application/a/b+json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}", Nothing)?
    # RFC 9110 allows empty parameters; a parameter without '=' is malformed.
    serve(&app, "POST /json HTTP/1.1\r\nHost: x\r\nContent-Type: application/json; \r\nContent-Length: 1\r\nConnection: close\r\n\r\n1", Nothing)?
    serve(&app, "POST /json HTTP/1.1\r\nHost: x\r\nContent-Type: application/json;; ;charset=utf-8;\r\nContent-Length: 1\r\nConnection: close\r\n\r\n2", Nothing)?
    serve(&app, "POST /json HTTP/1.1\r\nHost: x\r\nContent-Type: application/json; charset\r\nContent-Length: 1\r\nConnection: close\r\n\r\n3", Nothing)?
    app.json.max_input = 3
    serve(&app, "POST /json HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: 4\r\nConnection: close\r\n\r\nnull", Nothing)?
    Ok(())
"##,
        r#"
HTTP/1.1 200 OK||{"a":[1,2]}
HTTP/1.1 200 OK||true
HTTP/1.1 400 Bad Request||Bad Request
HTTP/1.1 415 Unsupported Media Type||Unsupported Media Type
HTTP/1.1 415 Unsupported Media Type||Unsupported Media Type
HTTP/1.1 200 OK||1
HTTP/1.1 200 OK||2
HTTP/1.1 415 Unsupported Media Type||Unsupported Media Type
HTTP/1.1 413 Content Too Large||Content Too Large
"#,
    );
}

/// Multipart helpers: `upload(parts)` wraps already formatted parts in a
/// request with boundary `XyZ`.
const UPLOAD: &str = r##"
def upload(parts: &str) -> Result[str, AllocError]:
    mut text = "POST /upload HTTP/1.1\r\nHost: x\r\nContent-Type: multipart/form-data; boundary=\"XyZ\"\r\nConnection: close\r\nContent-Length: "
    text = text.concat(str.from(parts.byte_len())?)?.concat("\r\n\r\n")?.concat(parts)?
    Ok(text)

def field(name: &str, value: &str) -> Result[str, AllocError]:
    Ok("--XyZ\r\nContent-Disposition: form-data; name=\"".concat(name)?.concat("\"\r\n\r\n")?.concat(value)?.concat("\r\n")?)

def file(name: &str, value: &str) -> Result[str, AllocError]:
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
def main() -> Result[(), std.bytes.BytesError | std.http.proxy.ProxyError | std.http.server.ServerError | AllocError | IoError | OverflowError[i64]]:
    mut app = app()?
    print(file_delta(app.baseline_files)?)?
    serve(&app, "POST /upload HTTP/1.1\r\nHost: x\r\nContent-Type: multipart/form-data; boundary=\"XyZ\"\r\nContent-Length: 208\r\nConnection: close\r\n\r\npreamble\r\n--XyZ\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nhello\r\n--XyZ  \r\ncontent-disposition: form-data; name=\"doc\"; filename=\"a b.txt\"\r\nContent-Type: text/csv\r\n\r\nx,y\r\n--XyXnot\r\n1,2\r\n--XyZ--\r\nepilogue", Nothing)?
    print(file_delta(app.baseline_files)?)?
    mut body = field("a", "")?
    body = body.concat(&file("b", "")?)?.concat("--XyZ--")?
    serve(&app, &upload(&body)?, Nothing)?
    serve(&app, &upload("--XyZ--")?, Nothing)?
    mut large = ""
    ten: Positive[i64] = 10
    for index in range(5000):
        large = large.concat("abcdefg")?.concat(str.from(index % ten)?)?
    mut long_name = ""
    for index in range(300):
        long_name = long_name.concat("n123456789")?
    body = file("big", &large)?.concat(&field(&long_name, "v")?)?.concat(&file("again", &large)?)?.concat("--XyZ--")?
    serve(&app, &upload(&body)?, Nothing)?
    Ok(())
"##
        ),
        r"
fds=0
fds=2
HTTP/1.1 200 OK||[title type=text/plain size=5 hello][doc file=a b.txt type=text/csv size=18 x,y|--XyXnot|1,2]
fds=0
fds=2
HTTP/1.1 200 OK||[a type=text/plain size=0 ][b file=f.txt type=text/csv size=0 ]
fds=0
HTTP/1.1 200 OK||
fds=4
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
def main() -> Result[(), std.bytes.BytesError | std.http.proxy.ProxyError | std.http.server.ServerError | AllocError | IoError | OverflowError[i64]]:
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
    print(file_delta(app.baseline_files)?)?
    serve(&app, &upload(&file("a", "unfinished")?)?, Nothing)?
    serve(&app, &upload("--XyZ\r\nContent-Disposition: attachment; name=\"a\"\r\n\r\nx\r\n--XyZ--")?, Nothing)?
    serve(&app, &upload("--XyZ\r\nContent-Type: text/plain\r\n\r\nx\r\n--XyZ--")?, Nothing)?
    serve(&app, &upload("--XyZ\r\nbroken header\r\n\r\nx\r\n--XyZ--")?, Nothing)?
    serve(&app, &upload("no delimiter at all")?, Nothing)?
    serve(&app, "POST /upload HTTP/1.1\r\nHost: x\r\nContent-Type: multipart/form-data\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", Nothing)?
    serve(&app, "POST /upload HTTP/1.1\r\nHost: x\r\nContent-Type: multipart/form-data; boundary=\"a@b\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", Nothing)?
    print(file_delta(app.baseline_files)?)?
    Ok(())
"##
        ),
        r"
HTTP/1.1 413 Content Too Large||Content Too Large
HTTP/1.1 413 Content Too Large||Content Too Large
HTTP/1.1 413 Content Too Large||Content Too Large
HTTP/1.1 413 Content Too Large||Content Too Large
HTTP/1.1 413 Content Too Large||Content Too Large
fds=0
HTTP/1.1 400 Bad Request||Bad Request
HTTP/1.1 400 Bad Request||Bad Request
HTTP/1.1 400 Bad Request||Bad Request
HTTP/1.1 400 Bad Request||Bad Request
HTTP/1.1 400 Bad Request||Bad Request
HTTP/1.1 400 Bad Request||Bad Request
HTTP/1.1 400 Bad Request||Bad Request
fds=0
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
def main() -> Result[(), std.bytes.BytesError | std.http.proxy.ProxyError | std.http.server.ServerError | AllocError | IoError | OverflowError[i64]]:
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
fds=0
HTTP/1.1 200 OK||[a type=text/plain size=1 1]
fds=0
HTTP/1.1 200 OK||[a type=text/plain size=1 1]
fds=0
HTTP/1.1 200 OK||[a type=text/plain size=1 1]
fds=0
HTTP/1.1 200 OK||[a type=text/plain size=1 1]
fds=0
HTTP/1.1 200 OK||[a type=text/plain size=1 1]
fds=0
HTTP/1.1 200 OK||[a type=text/plain size=1 1]
fds=0
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
def main() -> Result[(), std.bytes.BytesError | std.http.proxy.ProxyError | std.http.server.ServerError | AllocError | IoError | OverflowError[i64]]:
    mut app = app()?
    print(file_delta(app.baseline_files)?)?
    mut text = "POST /upload HTTP/1.1\r\nHost: x\r\nContent-Type: multipart/form-data; boundary=XyZ\r\nContent-Length: 1000\r\n\r\n"
    text = text.concat(&file("a", "partial content")?)?
    play(&app, &text, Nothing, True)?
    print(file_delta(app.baseline_files)?)?
    Ok(())
"##
        ),
        r"
fds=0
HTTP/1.1 408 Request Timeout||Request Timeout
fds=0
",
    );
}

/// Extra descriptors must survive both execs without changing the live-upload
/// deltas or masking descriptors leaked by a completed request (issue #299).
#[cfg(target_os = "linux")]
#[test]
fn multipart_checks_allow_inherited_descriptors() {
    use std::os::unix::process::CommandExt;

    unsafe extern "C" {
        fn dup(fd: i32) -> i32;
    }

    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "test_std_http_helpers::multipart_fields_and_files_are_parsed",
            "test_std_http_helpers::multipart_limits_and_malformed_bodies_leave_no_files",
            "test_std_http_helpers::multipart_header_of_exactly_max_head_is_accepted_on_any_read_boundary",
            "test_std_http_helpers::a_cancelled_upload_releases_its_file",
            "--test-threads=1",
        ])
        .stdin(std::process::Stdio::null());
    // dup is async-signal-safe and clears close-on-exec on the new descriptors.
    // Run only in the child so parallel tests never inherit these extra handles.
    unsafe {
        command.pre_exec(|| {
            if dup(0) < 0 || dup(0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let output = command.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("4 passed; 0 failed"), "{stdout}");
}

#[test]
fn cookies_are_read_leniently_and_written_strictly() {
    runs(
        r##"
def main() -> Result[(), std.bytes.BytesError | std.http.cookie.CookieError | std.http.proxy.ProxyError | std.http.server.ServerError | AllocError | IoError | OverflowError[i64]]:
    mut app = app()?
    serve(&app, "GET /cookies HTTP/1.1\r\nHost: x\r\nCookie: a=1; b=\"two\"; bad name=3; c=x,y; =4; d\r\nCookie: e=\r\nConnection: close\r\n\r\n", Nothing)?
    serve(&app, "GET /cookies HTTP/1.1\r\nHost: x\r\nCookie: a=1; b=2; c=3; d=4; e=5\r\nConnection: close\r\n\r\n", Nothing)?
    mut evil = std.http.cookie.set_cookie("id", "x")?
    evil.value = "x\r\nSet-Cookie: admin=1"
    print(evil.to_string().is_err())?
    evil.value = "x; Domain=evil.example"
    print(evil.to_string().is_err())?
    evil.value = "x"
    evil.name = "id\r\nX: y"
    print(evil.to_string().is_err())?
    evil.name = "id"
    evil.path = "/\r\nX: y"
    print(evil.to_string().is_err())?
    evil.path = "/a;b"
    print(evil.to_string().is_err())?
    evil.path = "/"
    evil.domain = "evil.example; Secure"
    print(evil.to_string().is_err())?
    print(std.http.cookie.set_cookie("a b", "x").is_err())?
    print(std.http.cookie.set_cookie("a", "\"x\"").is_err())?
    mut host = std.http.cookie.set_cookie("__Host-id", "x")?
    print(host.to_string()?)?
    host.domain = "example.com"
    print(host.to_string().is_err())?
    mut secure = std.http.cookie.set_cookie("__Secure-id", "x")?
    secure.secure = False
    print(secure.to_string().is_err())?
    mut cross = std.http.cookie.set_cookie("x", "y")?
    cross.same_site = SameSite.Unrestricted
    print(cross.to_string()?)?
    cross.secure = False
    print(cross.to_string().is_err())?
    cross.same_site = SameSite.Strict
    cross.path = ""
    cross.domain = "a.example"
    cross.domain = ".invalid"
    print(cross.to_string().is_err())?
    cross.domain = "a.example"
    cross.max_age = Nothing
    cross.http_only = False
    print(cross.to_string()?)?
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
def main() -> Result[(), std.bytes.BytesError | std.net.NetError | std.http.proxy.ProxyError | std.http.server.ServerError | AllocError | IoError | OverflowError[i64]]:
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
def made() -> Result[std.http.cookie.SetCookie, std.http.cookie.CookieError | AllocError | IoError]:
    name = "a-cookie-name".concat("-too-long-for-inline-storage")?
    value = "a-cookie-value".concat("-too-long-for-inline-storage")?
    print("__test_begin_no_allocations__")?
    cookie = std.http.cookie.set_cookie(&name, &value)
    print("__test_end_no_allocations__")?
    Ok(cookie?)

def main() -> Result[(), std.http.cookie.CookieError | AllocError | IoError]:
    cookie = made()?
    print(cookie.name, cookie.value)?
    print(std.http.cookie.set_cookie("", "x").is_err())?
    Ok(())
"##,
        "a-cookie-name-too-long-for-inline-storage a-cookie-value-too-long-for-inline-storage\nTrue",
    );
}

/// A transport that plays its input at once and discards what is written,
/// and a handler that prints one line per request: `.` and the first field
/// name or nothing on success, else the `BodyError` variant's initial.
const PROBE: &str = r##"
pub class Quiet:
    pub input: Bytes
    pub at: i64

    pub def read_before(self: &mut Quiet, buffer: &mut Bytes, start: i64, stop: i64, deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, NetError]:
        # Validated transport ranges and 0 <= at <= len(input); count is
        # bounded by both remaining ranges, so advancing stays within input.
        unchecked:
            mut count = stop - start
            if count > self.input.len() - self.at:
                count = self.input.len() - self.at
            buffer.overwrite(start, &self.input[self.at:self.at + count])
            self.at += count
            Ok(count)

    pub def shutdown(self: &mut Quiet, direction: Shutdown) -> Result[(), NetError]:
        Ok(())

    pub def write_before(self: &mut Quiet, source: &[u8], deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, NetError]:
        Ok(len(source))

def tag(error: &std.http.body.BodyError) -> str:
    match error:
        case std.http.body.BodyError.Malformed:
            "M"
        case std.http.body.BodyError.InvalidUtf8:
            "I"
        case std.http.body.BodyError.UnsupportedMediaType:
            "U"
        case _:
            "?"

pub class Probe:
    pub app: App

    pub def handle(self, route: &Route, exchange: &mut Exchange[Quiet]) -> Result[(), std.http.HttpError | AllocError | IoError]:
        match route:
            case Route.Form:
                match std.http.body.read_form(exchange, &self.app.form):
                    case Ok(_):
                        print(".")?
                    case Err(error):
                        print(tag(&error))?
            case _:
                match std.http.body.read_multipart(exchange, &self.app.multipart, &self.app.storage):
                    case Ok(parts):
                        print(".".concat(&parts[0].name)?)?
                    case Err(error):
                        print(tag(&error))?
        exchange.send_text(200, "")

# `prefix`, the byte `code`, then `suffix`, as one request; then `/`.
def probe(handler: &Probe, prefix: &str, code: i64, suffix: &str) -> Result[(), std.bytes.BytesError | std.http.server.ServerError | AllocError | IoError]:
    mut data = std.bytes.from_text(prefix)?
    data.push(u8(code))?
    data.extend(&std.bytes.from_text(suffix)?)?
    table = routes()?
    stop = CancellationToken()?
    cancel = CancellationToken()?
    report = std.http.server.serve_connection_from(Quiet(data, 0), Nothing, &table, handler, &std.http.server.config(), &stop, &cancel)
    print("/")?
    Ok(())

def form_sweep(handler: &Probe, before: &str, after: &str) -> Result[(), std.bytes.BytesError | std.http.server.ServerError | AllocError | IoError]:
    for code in range(256):
        probe(handler, &"POST /form HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\nContent-Type: ".concat(before)?, code, &after.concat("\r\n\r\n")?)?
    Ok(())

def part_sweep(handler: &Probe, before: &str, after: &str) -> Result[(), std.bytes.BytesError | std.http.server.ServerError | AllocError | IoError | OverflowError[i64]]:
    for code in range(256):
        body_size = 21 + before.byte_len() + after.byte_len() + 1
        mut head = "POST /upload HTTP/1.1\r\nHost: x\r\nContent-Type: multipart/form-data; boundary=XyZ\r\nContent-Length: "
        head = head.concat(str.from(body_size)?)?.concat("\r\n\r\n--XyZ\r\n")?.concat(before)?
        probe(handler, &head, code, &after.concat("\r\n\r\nv\r\n--XyZ--")?)?
    Ok(())

# Issue #383: a quoted boundary `a`, the byte `code`, `b`, used by the body.
def boundary_sweep(handler: &Probe) -> Result[(), std.bytes.BytesError | std.http.server.ServerError | AllocError | IoError]:
    for code in range(256):
        mut data = std.bytes.from_text("POST /upload HTTP/1.1\r\nHost: x\r\nContent-Length: 59\r\nContent-Type: multipart/form-data; boundary=\"a")?
        data.push(u8(code))?
        data.extend(&std.bytes.from_text("b\"\r\n\r\n--a")?)?
        data.push(u8(code))?
        data.extend(&std.bytes.from_text("b\r\nContent-Disposition: form-data; name=f\r\n\r\nv\r\n--a")?)?
        data.push(u8(code))?
        data.extend(&std.bytes.from_text("b--")?)?
        table = routes()?
        stop = CancellationToken()?
        cancel = CancellationToken()?
        report = std.http.server.serve_connection_from(Quiet(data, 0), Nothing, &table, handler, &std.http.server.config(), &stop, &cancel)
        print("/")?
    Ok(())

def cookie_sweep(before: &str, after: &str) -> Result[(), std.http.cookie.CookieError | AllocError | IoError]:
    for code in range(128):
        field = before.concat(&chr(u32(code)).unwrap())?.concat(after)?
        cookie_line(&field)?
    Ok(())

def cookie_line(field: &str) -> Result[(), std.http.cookie.CookieError | AllocError | IoError]:
    mut headers = list[std.http.Header].new()?
    headers.append(std.http.Header("Cookie", field.concat("")?))?
    mut text = "cookies "
    for cookie in std.http.cookie.cookies(&headers, 8)?:
        text = text.concat("[")?.concat(&cookie.name)?.concat("=")?.concat(&cookie.value)?.concat("]")?
    print(text)?
    Ok(())
"##;

fn tchar(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}

/// RFC 6265 cookie-octet.
fn cookie_octet(byte: u8) -> bool {
    (b'!'..=b'~').contains(&byte) && !b"\",;\\".contains(&byte)
}

/// A byte an HTTP field value may hold on its own: tab and printable ASCII.
fn field_byte(byte: u8) -> bool {
    byte == b'\t' || (b' '..=b'~').contains(&byte)
}

/// Issue #376: the bulk scans behind media-type parameters, multipart part
/// heads and Cookie fields accept exactly the RFC token, qdtext and
/// cookie-octet sets for every byte value, and report the same error variant
/// as the per-byte loops they replace. `H` marks a request that `std.http`
/// rejects before the body helpers see it.
#[test]
fn body_and_cookie_tokens_accept_exactly_the_rfc_sets() {
    let main = format!(
        r##"{PROBE}
def main() -> Result[(), std.bytes.BytesError | std.http.cookie.CookieError | std.http.proxy.ProxyError | std.http.server.ServerError | AllocError | IoError | OverflowError[i64]]:
    handler = Probe(app()?)
    form_sweep(&handler, "application/x-www-form-urlencoded; a", "b=c")?
    form_sweep(&handler, "application/x-www-form-urlencoded; a=b", "c")?
    form_sweep(&handler, "application/x-www-form-urlencoded", "; a=b")?
    part_sweep(&handler, "X-", "a: v\r\nContent-Disposition: form-data; name=f")?
    part_sweep(&handler, "X-a: v", "w\r\nContent-Disposition: form-data; name=f")?
    part_sweep(&handler, "Content-Disposition: form-data; name=f; x", "y=z")?
    part_sweep(&handler, "Content-Disposition: form-data; name=f", "g")?
    part_sweep(&handler, "Content-Disposition: form-data; name=\"f", "g\"")?
    part_sweep(&handler, "Content-Disposition: form-data; name=\"f\\", "g\"")?
    cookie_sweep("a", "b=1; k=v")?
    cookie_sweep("k=v; a=b", "c")?
    cookie_sweep("k=v; a=\"b", "c\"")?
    cookie_line("aéb=1; k=v")?
    cookie_line("k=v; a=béc")?
    Ok(())
"##
    );
    let output = run(&main);
    let mut lines = output.lines().peekable();
    let mut requests = Vec::new();
    let mut cookies = Vec::new();
    while let Some(line) = lines.next() {
        if let Some(found) = line.strip_prefix("cookies ") {
            cookies.push(found.to_string());
        } else if line == "/" {
            requests.push("H".to_string());
        } else {
            assert_eq!(lines.next(), Some("/"), "{line}");
            requests.push(line.to_string());
        }
    }
    let byte = |code: u8| char::from(code).to_string();
    let sweep = |tag: &dyn Fn(u8) -> String| (0..=255u8).map(tag).collect::<Vec<_>>();
    // Form Content-Type values pass `std.http` only as field content.
    let form = |accepted: &dyn Fn(u8) -> bool| {
        sweep(&|code| {
            if !field_byte(code) {
                "H".into()
            } else if accepted(code) {
                ".".into()
            } else {
                "U".into()
            }
        })
    };
    // Part heads are raw bytes: a lone byte above 127 is valid until the
    // value is converted to text.
    let part = |accepted: &dyn Fn(u8) -> bool, name: &dyn Fn(u8) -> String| {
        sweep(&|code| {
            if accepted(code) {
                format!(".{}", name(code))
            } else if code >= 128 {
                "I".into()
            } else {
                "M".into()
            }
        })
    };
    let fixed = |_| "f".to_string();
    let joined = |code| format!("f{}g", byte(code));
    let expected = [
        form(&tchar),
        form(&tchar),
        form(&|code| b"; \t".contains(&code)),
        sweep(&|code| {
            if tchar(code) || code == b':' {
                ".f".into()
            } else {
                "M".into()
            }
        }),
        part(&field_byte, &fixed),
        part(&tchar, &fixed),
        part(&tchar, &joined),
        // A backslash escapes the `g` after it.
        part(&|code| field_byte(code) && code != b'"', &|code| {
            if code == b'\\' {
                "fg".into()
            } else {
                joined(code)
            }
        }),
        part(&field_byte, &joined),
    ]
    .concat();
    assert_eq!(requests.len(), expected.len());
    for (index, (found, wanted)) in requests.iter().zip(&expected).enumerate() {
        assert_eq!(found, wanted, "sweep {} byte {}", index / 256, index % 256);
    }
    let cookie_sweep = |tag: &dyn Fn(u8) -> String| (0..128u8).map(tag).collect::<Vec<_>>();
    let expected = [
        cookie_sweep(&|code| match code {
            _ if tchar(code) => format!("[a{}b=1][k=v]", byte(code)),
            b'=' => "[a=b=1][k=v]".into(),
            b';' => "[b=1][k=v]".into(),
            _ => "[k=v]".into(),
        }),
        cookie_sweep(&|code| match code {
            _ if cookie_octet(code) => format!("[k=v][a=b{}c]", byte(code)),
            b';' => "[k=v][a=b]".into(),
            _ => "[k=v]".into(),
        }),
        cookie_sweep(&|code| {
            if cookie_octet(code) {
                format!("[k=v][a=b{}c]", byte(code))
            } else {
                "[k=v]".into()
            }
        }),
        vec!["[k=v]".into(), "[k=v]".into()],
    ]
    .concat();
    assert_eq!(cookies.len(), expected.len());
    for (index, (found, wanted)) in cookies.iter().zip(&expected).enumerate() {
        assert_eq!(
            found,
            wanted,
            "cookie sweep {} byte {}",
            index / 128,
            index % 128
        );
    }
}

/// Issue #383: the boundary check is a set scan; a quoted boundary accepts
/// exactly the RFC 2046 bchars. `H` marks a request `std.http` rejects first.
#[test]
fn multipart_boundaries_accept_exactly_bchars() {
    let main = format!(
        r##"{PROBE}
def main() -> Result[(), std.bytes.BytesError | std.http.proxy.ProxyError | std.http.server.ServerError | AllocError | IoError | OverflowError[i64]]:
    boundary_sweep(&Probe(app()?))?
    Ok(())
"##
    );
    let output = run(&main);
    let mut lines = output.lines().peekable();
    let mut requests = Vec::new();
    while let Some(line) = lines.next() {
        if line == "/" {
            requests.push("H".to_string());
        } else {
            assert_eq!(lines.next(), Some("/"), "{line}");
            requests.push(line.to_string());
        }
    }
    let bchar = |code: u8| code.is_ascii_alphanumeric() || b" '()+,-./:=?_".contains(&code);
    let expected: Vec<String> = (0..=255u8)
        .map(|code| {
            if !field_byte(code) {
                "H".into()
            } else if code == b'"' {
                // The quoted string ends early, leaving a malformed media type.
                "U".into()
            } else if bchar(code) {
                ".f".into()
            } else {
                "M".into()
            }
        })
        .collect();
    assert_eq!(requests.len(), expected.len());
    for (code, (found, wanted)) in requests.iter().zip(&expected).enumerate() {
        assert_eq!(found, wanted, "byte {code}");
    }
}

/// Issue #376: the token, qdtext, control and cookie-octet checks are bulk
/// scans, not a classifier call per byte.
#[test]
fn body_and_cookie_classes_are_bulk_scans() {
    let dir = tempfile::tempdir().unwrap();
    let source = format!(
        "{}def main() -> Result[(), std.http.proxy.ProxyError | IoError | OverflowError[i64]]:\n    app()?\n    Ok(())\n",
        PRELUDE.replace("$STORAGE", "/tmp")
    );
    std::fs::write(dir.path().join("main.plenty"), source).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_plenty"))
        .current_dir(dir.path())
        .env("PLENTY_CACHE", "off")
        .args([
            "--release",
            "--dump",
            "clif",
            "--dump-std",
            "--dump-filter",
            "std.http.",
            "main.plenty",
            "-o",
            "-",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let clif = String::from_utf8(output.stdout).unwrap();
    let function = |name: &str| {
        let start = clif
            .find(&format!("==== {name} "))
            .unwrap_or_else(|| panic!("{name} missing"));
        let rest = &clif[start + 5..];
        rest[..rest.find("\n==== ").unwrap_or(rest.len())].to_string()
    };
    // The scan helpers (`token_end`, `quoted_end`, `has_control`, `all_in`)
    // may be called or lowered in these callers (issue #722); either way no
    // per-byte classifier is called.
    for (name, absent) in [
        ("std.http.body.parameters", &["token_byte"][..]),
        ("std.http.body.part_head", &["token_byte"]),
        (
            "std.http.cookie.split_pairs",
            &["valid_name", "valid_value", "token_byte"],
        ),
    ] {
        let body = function(name);
        for call in absent {
            assert!(!body.contains(call), "{name} calls {call}:\n{body}");
        }
    }
}

/// Compile boundary probes with explicit optimization and runtime test hooks.
fn checked_consumer_output(source: &str, optimize: bool) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable_with_options(
        &path,
        &executable,
        None,
        &plenty::CompileOptions {
            optimize,
            test_hooks: true,
            ..Default::default()
        },
    )
    .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{:?}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn body_size_helpers_preserve_too_large_at_representation_boundary() {
    fn helper<'a>(source: &'a str, name: &str) -> &'a str {
        let start = source.find(&format!("def {name}(")).unwrap();
        source[start..].split("\n\n").next().unwrap()
    }
    let body = include_str!("../stdlib/std/http/body.plenty");
    let source = format!(
        r#"import std.http
import std.bytes
import std.fs
import std.http.body
from std.http import Header
from std.http.body import BodyError, MultipartLimits
from std.bytes import Bytes
from std.fs import TempFile
from std.io import StreamError
import std.error
import std.io
{allocated}

{storage_failure}

{emit}

{extent}

{decimal}

{digit}

{declared}

def main() -> Result[(), AllocError | IoError]:
    print("__test_begin_no_allocations__")?
    exact = body_extent(9223372036854775800, 7).unwrap() == i64.MAX
    overflow = match body_extent(i64.MAX, 1):
        case Err(BodyError.TooLarge):
            True
        case _:
            False
    decimal_exact = decimal_extent(922337203685477580, 55).unwrap() == i64.MAX
    decimal_over = match decimal_extent(922337203685477580, 56):
        case Err(BodyError.TooLarge):
            True
        case _:
            False
    print("__test_end_no_allocations__")?
    print(exact, overflow, decimal_exact, decimal_over)?
    headers = [Header("Content-Length", "9223372036854775807")]?
    print(declared_within(&headers, i64.MAX).is_ok())?
    too_long = [Header("Content-Length", "9223372036854775808")]?
    rejected = match declared_within(&too_long, i64.MAX):
        case Err(BodyError.TooLarge):
            True
        case _:
            False
    print(rejected)?
    mut text = std.bytes.from_text("held").unwrap()
    mut file: Option[TempFile] = Nothing
    limits = std.http.body.MultipartLimits(1, 9223372036854775807, 9223372036854775807, 9223372036854775807, 9223372036854775807)
    rejected_emit = match emit(b"x", 0, 1, &mut file, &mut text, i64.MAX, &limits):
        case Err(BodyError.TooLarge):
            True
        case _:
            False
    print(rejected_emit, text.len() == 4)?
    Ok(())
"#,
        allocated = helper(body, "allocated[T]"),
        storage_failure = helper(body, "storage_failure"),
        emit = helper(body, "emit"),
        extent = helper(body, "body_extent"),
        decimal = helper(body, "decimal_extent"),
        digit = helper(body, "digit"),
        declared = helper(body, "declared_within")
    );
    for optimize in [false, true] {
        assert_eq!(
            checked_consumer_output(&source, optimize),
            "True True True True\nTrue\nTrue\nTrue True"
        );
    }
}

#[test]
fn body_forms_and_cookie_extreme_age_and_signatures() {
    let source = r#"import std.bytes
import std.time
import std.http
import std.http.body
import std.http.cookie
from std.http import Header

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | std.http.body.BodyError | std.http.cookie.CookieError]:
    limits = std.http.body.FormLimits(9223372036854775807, 9223372036854775807)
    for text in ["", "a", "a=", "a=%", "a=%0", "a=%00&", "&&a=+%41&&"]?:
        result = std.http.body.parse_form_text(text, &limits)
        match result:
            case Ok(pairs):
                print(len(pairs))?
            case Err(error):
                print(std.http.body.status(&error))?
    headers = [Header("Cookie", ""), Header("Cookie", "a=; b=\"\"; bad name=x;" )]?
    print(len(std.http.cookie.cookies(&headers, 2)?))?
    print(std.http.cookie.cookies(&headers, 1).is_err())?
    mut cookie = std.http.cookie.set_cookie("id", "x")?
    cookie.max_age = Some(std.time.nanoseconds(u64.MAX))
    print("Max-Age=18446744074;" in cookie.to_string()?)?
    keys = [std.bytes.from_text("01234567890123456789012345678901")?]?
    signed = std.http.cookie.sign(&keys, "id", "value")?
    print(std.http.cookie.verify(&keys, "id", &signed)?.unwrap())?
    print(std.http.cookie.verify(&keys, "id", "")?.is_none())?
    print(std.http.cookie.verify(&keys, "id", ".")?.is_none())?
    print(std.http.cookie.verify(&keys, "other", &signed)?.is_none())?
    Ok(())
"#;
    for optimize in [false, true] {
        assert_eq!(
            checked_consumer_output(source, optimize),
            "0\n1\n1\n400\n400\n1\n1\n2\nTrue\nTrue\nvalue\nTrue\nTrue\nTrue"
        );
    }
}

#[test]
fn body_multipart_reads_parts_and_rejects_limits_through_server() {
    let storage = tempfile::tempdir().unwrap();
    let source = r#"import std.bytes
import std.fs
import std.http
import std.http.body
import std.http.server
import std.net
import std.path
from std.bytes import Bytes
from std.net import NetError, Shutdown
from std.sync import CancellationToken
from std.time import Deadline

pub class Input:
    data: Bytes
    at: i64

    pub def read_before(self: &mut Input, buffer: &mut Bytes, start: i64, stop: i64, deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, NetError]:
        # Transport ranges are validated; 0 <= at <= len(data); take <= both
        # remaining ranges, so subtraction and advancing cannot overflow.
        unchecked:
            mut take = stop - start
            if take > 7:
                take = 7
            if take > self.data.len() - self.at:
                take = self.data.len() - self.at
            buffer.overwrite(start, &self.data[self.at:self.at + take])
            self.at += take
            Ok(take)

    pub def write_before(self: &mut Input, source: &[u8], deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, NetError]:
        Ok(len(source))

    pub def shutdown(self: &mut Input, direction: Shutdown) -> Result[(), NetError]:
        Ok(())

pub class Handler:
    root: std.fs.Root
    limits: std.http.body.MultipartLimits

    pub def handle(self, route: &i64, exchange: &mut std.http.server.Exchange[Input]) -> Result[(), std.http.HttpError | IoError]:
        if *route == 1:
            match std.http.body.read_form(exchange, &std.http.body.FormLimits(3, 2)):
                case Ok(pairs):
                    print(len(pairs))?
                case Err(error):
                    print(std.http.body.status(&error))?
            return exchange.send_text(200, "done")
        match std.http.body.read_multipart(exchange, &self.limits, &self.root):
            case Ok(parts):
                print(len(parts))?
                for part in parts:
                    print(part.name, part.size)?
            case Err(error):
                print(std.http.body.status(&error))?
        exchange.send_text(200, "done")

def main() -> Result[(), AllocError | IoError | std.http.server.ServerError]:
    mut router = std.http.server.router[i64]()?
    router.add("POST", "/", 0)?
    router.add("POST", "/form", 1)?
    mut handler = Handler(std.fs.root(&std.path.from_text("$STORAGE")?)?, std.http.body.multipart_limits())
    handler.limits.max_head = 9223372036854775807
    body = "--B\r\nContent-Disposition: form-data; name=field\r\n\r\nabc\r\n--B\r\nContent-Disposition: form-data; name=file; filename=x\r\n\r\ndef\r\n--B--"
    head = "POST / HTTP/1.1\r\nHost: x\r\nConnection: close\r\nContent-Type: multipart/form-data; boundary=B\r\nContent-Length: ".concat(str.from(body.byte_len())?)?.concat("\r\n\r\n")?
    request = head.concat(body)?
    cancel = CancellationToken()?
    stop = CancellationToken()?
    config = std.http.server.config()
    for limit in [3, 2]?:
        handler.limits.max_file = NonNegative.try_from(limit).unwrap()
        transport = Input(std.bytes.from_text(&request).unwrap(), 0)
        report = std.http.server.serve_connection(transport, &router, &handler, &config, &stop, &cancel)
        print(report.handler_errors)?
    for form in ["Content-Length: 3\r\n\r\na=1", "Content-Length: 4\r\n\r\na=12", "Transfer-Encoding: chunked\r\n\r\n4\r\na=12\r\n0\r\n\r\n"]?:
        form_request = "POST /form HTTP/1.1\r\nHost: x\r\nConnection: close\r\nContent-Type: application/x-www-form-urlencoded\r\n".concat(form)?
        report = std.http.server.serve_connection(Input(std.bytes.from_text(&form_request).unwrap(), 0), &router, &handler, &config, &stop, &cancel)
        print(report.handler_errors)?
    Ok(())
"#
    .replace("$STORAGE", storage.path().to_str().unwrap());
    for optimize in [false, true] {
        assert_eq!(
            checked_consumer_output(&source, optimize),
            "2\nfield 3\nfile 3\n0\n413\n0\n1\n0\n413\n0\n413\n0"
        );
        assert_eq!(std::fs::read_dir(storage.path()).unwrap().count(), 0);
    }
}

#[test]
fn proxy_cursor_and_prefix_helpers_cover_integer_boundaries() {
    fn helper<'a>(source: &'a str, name: &str) -> &'a str {
        let start = source.find(&format!("def {name}(")).unwrap();
        source[start..].split("\n\n").next().unwrap()
    }
    let proxy = include_str!("../stdlib/std/http/proxy.plenty");
    let source = format!(
        r#"from std.http.proxy import ProxyError
{mask}

{escape}

def main() -> Result[(), IoError]:
    print("__test_begin_no_allocations__")?
    minimum = mask64(i64.MIN) == 0u64
    maximum = mask64(i64.MAX) == u64.MAX
    zero = mask64(0) == 0u64
    one = mask64(1) == 9223372036854775808u64
    almost_full = mask64(63) == 18446744073709551614u64
    full = mask64(64) == u64.MAX
    exact = escaped_end(9223372036854775805, i64.MAX).unwrap() == i64.MAX
    dangling = match escaped_end(9223372036854775806, i64.MAX):
        case Err(ProxyError.InvalidForwarded):
            True
        case _:
            False
    tiny = escaped_end(0, 1).is_err()
    print("__test_end_no_allocations__")?
    print(minimum, maximum, zero, one, almost_full, full, exact, dangling, tiny)?
    Ok(())
"#,
        mask = helper(proxy, "mask64"),
        escape = helper(proxy, "escaped_end")
    );
    for optimize in [false, true] {
        assert_eq!(
            checked_consumer_output(&source, optimize),
            "True True True True True True True True True"
        );
    }
}

#[test]
fn proxy_forwarding_boundaries_preserve_errors_and_trust() {
    let source = r#"import std.http.proxy
import std.net
from std.http import Header
from std.http.proxy import ProxyError, Forwarding

def main() -> Result[(), ProxyError | std.net.NetError | AllocError | IoError]:
    mut proxy = std.http.proxy.proxies(Forwarding.Forwarded)?
    proxy.trust("0.0.0.0/0")?
    proxy.trust("::/0")?
    print(proxy.trust("0.0.0.0/999").is_err(), proxy.trust("0.0.0.0/0000").is_err())?
    proxy.trust("255.255.255.255/32")?
    proxy.trust("ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff/128")?
    peer = std.net.parse_address("10.0.0.1:443")?
    for value in ["", ",,,", "for=192.0.2.1,", "for=\"192.0.2.1\"", "for=\"192.0.2.1\\\"", "for=\"192.0.2.1\\", "for=192.0.2.1;proto=HTTPS"]?:
        headers = [Header("Forwarded", value)]?
        match std.http.proxy.client(&peer, &headers, &proxy):
            case Ok(client):
                print(std.net.format_ip(client.address)?, client.scheme, client.proxied)?
            case Err(ProxyError.InvalidForwarded):
                print("invalid")?
            case Err(_):
                print("unexpected error")?
    proxy.max_hops = 0
    empty = [Header("Forwarded", ",,,")]?
    print(std.http.proxy.client(&peer, &empty, &proxy)?.proxied)?
    one = [Header("Forwarded", "for=192.0.2.1")]?
    too_many = match std.http.proxy.client(&peer, &one, &proxy):
        case Err(ProxyError.TooManyHops):
            True
        case _:
            False
    print(too_many)?
    proxy.header = Forwarding.XForwardedFor
    proxy.max_hops = 9223372036854775807
    nodes = [Header("X-Forwarded-For", ", 192.0.2.1,,10.0.0.2, "), Header("X-Forwarded-Proto", ", https,")]?
    print(std.net.format_ip(std.http.proxy.client(&peer, &nodes, &proxy)?.address)?)?
    Ok(())
"#;
    for optimize in [false, true] {
        assert_eq!(
            checked_consumer_output(source, optimize),
            "True True\n10.0.0.1 Option[str].Nothing False\n10.0.0.1 Option[str].Nothing False\n192.0.2.1 Option[str].Nothing True\n192.0.2.1 Option[str].Nothing True\ninvalid\ninvalid\n192.0.2.1 Option[str].Some(\"https\") True\nFalse\nTrue\n192.0.2.1"
        );
    }
}
