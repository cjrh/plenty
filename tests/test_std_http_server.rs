//! `std.http.server`: routing, workers, admission, deadlines and shutdown.
//! Scripted connections run in memory through `serve_connection`; the rest
//! use loopback sockets on ephemeral ports. No test needs a network.

/// An in-memory `Transport` and a handler covering each response style.
const SCRIPTED: &str = r##"import std.bytes
import std.http
import std.http.server
import std.net
import std.time
from std.bytes import Bytes
from std.http import Header, HttpError
from std.http.server import Config, Exchange, Report, Router, ServerError, Transport
from std.net import NetError, TcpStream
from std.sync import CancellationToken
from std.time import Deadline

# Plays `input` to the server `step` bytes at a time and, when `echo` is set,
# copies what the server writes to standard output. Once the input runs out it reports a
# clean end, or a timeout when `hang` is set.
pub class Script:
    pub input: Bytes
    pub at: i64
    pub step: i64
    pub hang: bool
    pub echo: bool

    pub def read_before(self: &mut Script, buffer: &mut Bytes, start: i64, stop: i64, deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, NetError]:
        if cancel.is_cancelled():
            return Err(NetError.Cancelled)
        left = self.input.len() - self.at
        if left == 0:
            if self.hang:
                return Err(NetError.TimedOut)
            return Ok(0)
        mut count = stop - start
        if count > self.step:
            count = self.step
        if count > left:
            count = left
        buffer.overwrite(start, &self.input, self.at, self.at + count).unwrap()
        self.at += count
        Ok(count)

    pub def write_before(self: &mut Script, source: &Bytes, start: i64, stop: i64, deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, NetError]:
        if cancel.is_cancelled():
            return Err(NetError.Cancelled)
        if self.echo:
            source.write_stdout(start, stop).unwrap()
        Ok(stop - start)

pub class App:
    pub label: str

    pub def handle(self, exchange: &mut Exchange[Script]) -> Result[(), Failure]:
        answer(&self.label, exchange)

pub class NetApp:
    pub label: str

    pub def handle(self, exchange: &mut Exchange[TcpStream]) -> Result[(), Failure]:
        answer(&self.label, exchange)

pub def answer[T: Transport](label: &str, exchange: &mut Exchange[T]) -> Result[(), Failure]:
        route = exchange.route
        if route == 0:
            exchange.send_text(200, label)?
        elif route == 1:
            param = exchange.params[0].concat("|")?
            exchange.send_text(200, &param)?
        elif route == 2:
            mut body = std.bytes.empty()?
            exchange.read_all(&mut body)?
            exchange.send(200, "application/octet-stream", &body)?
        elif route == 3:
            return Err(Failure.Unspecified)
        elif route == 4:
            exchange.start(std.http.response(200, "OK")?)?
            piece = std.bytes.from_text("abcdef")?
            exchange.write(&piece, 0, 2)?
            exchange.write(&piece, 2, 6)?
        elif route == 5:
            pass
        elif route == 6:
            mut head = std.http.response(200, "OK")?
            head.add("Content-Length", "5")?
            exchange.start(head)?
            piece = std.bytes.from_text("hello")?
            exchange.write(&piece, 0, 5)?
            exchange.end()?
        elif route == 7:
            query = exchange.query.concat("")?
            exchange.send_text(200, &query)?
        elif route == 8:
            exchange.start(std.http.response(200, "OK")?)?
            piece = std.bytes.from_text("part")?
            exchange.write(&piece, 0, 4)?
            return Err(Failure.Unspecified)
        elif route == 9:
            authority = exchange.authority.concat("|")?
            exchange.send_text(200, &authority)?
        elif route == 10:
            # Answering gives the connection to the response, so the body
            # is gone; reads report `Broken` without choosing a refusal.
            exchange.send_text(200, "sent")?
            mut buffer = std.bytes.from_text("....")?
            mut out = std.bytes.empty()?
            match exchange.read_body(&mut buffer, 0, 4):
                case Err(HttpError.Broken):
                    pass
                case _:
                    return Err(Failure.Unspecified)
            match exchange.read_all(&mut out):
                case Err(HttpError.Broken):
                    pass
                case _:
                    return Err(Failure.Unspecified)
        Ok(())

pub def routes() -> Result[Router, ServerError]:
    mut table = std.http.server.router()?
    table.add("GET", "/", 0)?
    table.add("GET", "/hello/{name}", 1)?
    table.add("POST", "/echo", 2)?
    table.add("GET", "/fail", 3)?
    table.add("GET", "/stream", 4)?
    table.add("GET", "/silent", 5)?
    table.add("GET", "/sized", 6)?
    table.add("*", "/query", 7)?
    table.add("GET", "/broken", 8)?
    table.add("GET", "/files/{path...}", 1)?
    table.add("GET", "/authority", 9)?
    table.add("POST", "/late", 10)?
    Ok(table)

pub def counts(report: &Report) -> Result[str, Failure]:
    mut text = "connections=".concat(str.from(report.connections)?)?
    text = text.concat(" requests=")?.concat(str.from(report.requests)?)?
    text = text.concat(" handler_errors=")?.concat(str.from(report.handler_errors)?)?
    text = text.concat(" protocol_errors=")?.concat(str.from(report.protocol_errors)?)?
    text = text.concat(" timeouts=")?.concat(str.from(report.timeouts)?)?
    text = text.concat(" cancelled=")?.concat(str.from(report.cancelled)?)?
    text = text.concat(" rejected=")?.concat(str.from(report.rejected)?)?
    Ok(text)

# Serves one scripted connection and prints its counts.
pub def scripted(input: &str, step: i64, hang: bool, config: &Config) -> Result[(), Failure]:
    table = routes()?
    app = App("home")
    stop = CancellationToken()?
    cancel = CancellationToken()?
    script = Script(std.bytes.from_text(input)?, 0, step, hang, True)
    report = std.http.server.serve_connection(script, &table, &app, config, &stop, &cancel)
    print("")?
    print(counts(&report)?)?
    Ok(())
"##;

/// Loopback client helpers and a handler whose requests can be held open.
const LOOPBACK: &str = r##"import std.bytes
import std.http.server
import std.net
import std.time
import std.http
from std.bytes import Bytes
from std.http.server import Exchange, Report
from std.net import NetError, TcpStream
from std.sync import CancellationToken

# `entered` is cancelled when /wait starts; /wait then waits for `gate`.
pub class Gated:
    pub entered: CancellationToken
    pub gate: CancellationToken

    pub def handle(self, exchange: &mut Exchange[TcpStream]) -> Result[(), Failure]:
        if exchange.route == 0:
            exchange.send_text(200, "home")?
        elif exchange.route == 1:
            self.entered.cancel()
            drop(self.gate.wait_timeout(5000u64))
            exchange.send_text(200, "waited")?
        elif exchange.route == 2:
            # Streams until shutdown begins, then finishes politely.
            exchange.start(std.http.response(200, "OK")?)?
            tick = std.bytes.from_text("tick\n")?
            while not exchange.stopping():
                exchange.write(&tick, 0, tick.len())?
                std.time.sleep(&std.time.milliseconds(20u32))
            bye = std.bytes.from_text("bye\n")?
            exchange.write(&bye, 0, bye.len())?
        elif exchange.route == 3:
            # Ignores shutdown; only cancellation stops it.
            exchange.start(std.http.response(200, "OK")?)?
            tick = std.bytes.from_text("tick\n")?
            while True:
                exchange.write(&tick, 0, tick.len())?
                std.time.sleep(&std.time.milliseconds(20u32))
        Ok(())

pub def gated() -> Result[Gated, Failure]:
    Ok(Gated(CancellationToken()?, CancellationToken()?))

pub def routes() -> Result[std.http.server.Router, std.http.server.ServerError]:
    mut table = std.http.server.router()?
    table.add("GET", "/", 0)?
    table.add("GET", "/wait", 1)?
    table.add("GET", "/polite", 2)?
    table.add("GET", "/stubborn", 3)?
    Ok(table)

pub def connect_to(port: u16, raw: &str) -> Result[TcpStream, NetError]:
    never = CancellationToken().unwrap()
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(5u32)).unwrap()
    mut stream = std.net.connect(&std.net.loopback_v4(port), &deadline, &never)?
    request = std.bytes.from_text(raw).unwrap()
    stream.write_all_before(&request, 0, request.len(), &deadline, &never)?
    Ok(stream)

# Reads until `marker` has arrived, or to the end when it is empty.
pub def until(stream: &mut TcpStream, marker: &str) -> Result[str, NetError]:
    never = CancellationToken().unwrap()
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(5u32)).unwrap()
    mut out = std.bytes.empty().unwrap()
    while True:
        at = out.len()
        out.resize(at + 1, 0u8).unwrap()
        count = stream.read_before(&mut out, at, at + 1, &deadline, &never)?
        out.truncate(at + count).unwrap()
        if count == 0:
            break
        if marker.byte_len() > 0 and out.to_text().unwrap().endswith(marker):
            break
    Ok(out.to_text().unwrap().replace("\r\n", "|").unwrap())

pub def fetch(port: u16, raw: &str) -> Result[str, NetError]:
    mut stream = connect_to(port, raw)?
    until(&mut stream, "")

pub def counts(report: &Report) -> Result[str, Failure]:
    mut text = "connections=".concat(str.from(report.connections)?)?
    text = text.concat(" requests=")?.concat(str.from(report.requests)?)?
    text = text.concat(" handler_errors=")?.concat(str.from(report.handler_errors)?)?
    text = text.concat(" protocol_errors=")?.concat(str.from(report.protocol_errors)?)?
    text = text.concat(" timeouts=")?.concat(str.from(report.timeouts)?)?
    text = text.concat(" cancelled=")?.concat(str.from(report.cancelled)?)?
    text = text.concat(" rejected=")?.concat(str.from(report.rejected)?)?
    Ok(text)
"##;

fn output(source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let output = std::process::Command::new(executable)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{source}\nstatus {:?}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .replace("\r\n", "|")
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_owned()
}

fn runs(prelude: &str, main: &str, expected: &str) {
    assert_eq!(output(&format!("{prelude}{main}")), expected.trim_end());
}

/// Ten pipelined requests on one connection, delivered five bytes at a time:
/// literal, captured and rest routes, query text, a chunked request body,
/// HEAD, chunked and sized streams, and a handler that never answers.
#[test]
fn scripted_requests_are_routed_framed_and_pipelined() {
    runs(
        SCRIPTED,
        r##"
def main() -> Result[(), Failure]:
    config = std.http.server.config()
    scripted("GET / HTTP/1.1\r\nHost: x\r\n\r\nGET /hello/a%2Fb HTTP/1.1\r\nHost: x\r\n\r\nGET /files/x/y/z HTTP/1.1\r\nHost: x\r\n\r\nGET /files/ HTTP/1.1\r\nHost: x\r\n\r\nGET /query?a=1&b HTTP/1.1\r\nHost: x\r\n\r\nPOST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n0\r\n\r\nHEAD /stream HTTP/1.1\r\nHost: x\r\n\r\nGET /stream HTTP/1.1\r\nHost: x\r\n\r\nGET /sized HTTP/1.1\r\nHost: x\r\n\r\nGET /silent HTTP/1.1\r\nHost: x\r\n\r\n", 5, False, &config)?
    Ok(())
"##,
        r##"HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 4||homeHTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 4||a/b|HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 6||x/y/z|HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 1|||HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 5||a=1&bHTTP/1.1 200 OK|Content-Type: application/octet-stream|Content-Length: 3||abcHTTP/1.1 200 OK|Transfer-Encoding: chunked||HTTP/1.1 200 OK|Transfer-Encoding: chunked||2|ab|4|cdef|0||HTTP/1.1 200 OK|Content-Length: 5||helloHTTP/1.1 500 Internal Server Error|Content-Type: text/plain; charset=utf-8|Content-Length: 22||Internal Server Error

connections=1 requests=10 handler_errors=1 protocol_errors=0 timeouts=0 cancelled=0 rejected=0"##,
    );
}

/// Malformed heads, absolute-form targets, Host faults, unsupported framing,
/// `100 Continue`, HTTP/1.0, unread bodies, failures mid-stream, stalled
/// peers, 404/405, request limits and body limits, each on its own scripted
/// connection.
#[test]
fn faults_are_answered_and_counted() {
    runs(
        SCRIPTED,
        r##"
def main() -> Result[(), Failure]:
    mut config = std.http.server.config()
    print("--- bad")?
    scripted("GET / HTTP/1.1\r\nHost: x\r\nBad Header\r\n\r\n", 64, False, &config)?
    print("--- version")?
    scripted("GET / HTTP/2.0\r\n\r\n", 64, False, &config)?
    print("--- absolute")?
    scripted("GET hTTp://Example.com:8080/authority?q HTTP/1.1\r\nHost: other\r\n\r\nGET /authority HTTP/1.1\r\nHost: x\r\n\r\nGET /authority HTTP/1.0\r\n\r\n", 64, False, &config)?
    print("--- host")?
    scripted("GET / HTTP/1.1\r\n\r\n", 64, False, &config)?
    scripted("GET / HTTP/1.1\r\nHost: x\r\nhost: x\r\n\r\n", 64, False, &config)?
    scripted("GET / HTTP/1.0\r\nHost: a b\r\n\r\n", 64, False, &config)?
    scripted("GET http://u@x/ HTTP/1.1\r\nHost: x\r\n\r\n", 64, False, &config)?
    print("--- te")?
    scripted("POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: gzip\r\n\r\n", 64, False, &config)?
    print("--- continue")?
    scripted("POST /echo HTTP/1.1\r\nHost: x\r\nExpect: 100-continue\r\nContent-Length: 2\r\n\r\nhi", 64, False, &config)?
    print("--- http10")?
    scripted("GET /stream HTTP/1.0\r\n\r\nGET / HTTP/1.0\r\n\r\n", 64, False, &config)?
    print("--- keepalive10")?
    scripted("GET / HTTP/1.0\r\nConnection: keep-alive\r\n\r\nGET / HTTP/1.0\r\n\r\n", 64, False, &config)?
    print("--- unread")?
    scripted("POST /query HTTP/1.1\r\nHost: x\r\nContent-Length: 3\r\n\r\nabcGET / HTTP/1.1\r\nHost: x\r\n\r\n", 64, False, &config)?
    print("--- broken")?
    scripted("GET /broken HTTP/1.1\r\nHost: x\r\n\r\nGET / HTTP/1.1\r\nHost: x\r\n\r\n", 64, False, &config)?
    print("--- hang head")?
    scripted("GET / HTTP/1.1\r\nHost: x\r\nHost:", 64, True, &config)?
    print("--- hang idle")?
    scripted("GET / HTTP/1.1\r\nHost: x\r\n\r\n", 64, True, &config)?
    print("--- hang body")?
    scripted("POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 9\r\n\r\nabc", 64, True, &config)?
    print("--- 405 and 404")?
    scripted("DELETE /echo HTTP/1.1\r\nHost: x\r\n\r\nGET /missing HTTP/1.1\r\nHost: x\r\n\r\nGET /hello/%zz HTTP/1.1\r\nHost: x\r\n\r\n", 64, False, &config)?
    config.max_requests = 2
    config.limits.max_body = 4
    print("--- max")?
    scripted("GET / HTTP/1.1\r\nHost: x\r\n\r\nGET / HTTP/1.1\r\nHost: x\r\n\r\nGET / HTTP/1.1\r\nHost: x\r\n\r\n", 1, False, &config)?
    print("--- body limit")?
    scripted("POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\n\r\nhello", 64, False, &config)?
    scripted("POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n", 64, False, &config)?
    Ok(())
"##,
        r##"--- bad
HTTP/1.1 400 Bad Request|Content-Type: text/plain; charset=utf-8|Content-Length: 12|Connection: close||Bad Request

connections=1 requests=0 handler_errors=0 protocol_errors=1 timeouts=0 cancelled=0 rejected=0
--- version
HTTP/1.1 505 HTTP Version Not Supported|Content-Type: text/plain; charset=utf-8|Content-Length: 27|Connection: close||HTTP Version Not Supported

connections=1 requests=0 handler_errors=0 protocol_errors=1 timeouts=0 cancelled=0 rejected=0
--- absolute
HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 17||Example.com:8080|HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 2||x|HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 1|Connection: close|||
connections=1 requests=3 handler_errors=0 protocol_errors=0 timeouts=0 cancelled=0 rejected=0
--- host
HTTP/1.1 400 Bad Request|Content-Type: text/plain; charset=utf-8|Content-Length: 12|Connection: close||Bad Request

connections=1 requests=1 handler_errors=0 protocol_errors=1 timeouts=0 cancelled=0 rejected=0
HTTP/1.1 400 Bad Request|Content-Type: text/plain; charset=utf-8|Content-Length: 12|Connection: close||Bad Request

connections=1 requests=1 handler_errors=0 protocol_errors=1 timeouts=0 cancelled=0 rejected=0
HTTP/1.1 400 Bad Request|Content-Type: text/plain; charset=utf-8|Content-Length: 12|Connection: close||Bad Request

connections=1 requests=1 handler_errors=0 protocol_errors=1 timeouts=0 cancelled=0 rejected=0
HTTP/1.1 400 Bad Request|Content-Type: text/plain; charset=utf-8|Content-Length: 12|Connection: close||Bad Request

connections=1 requests=1 handler_errors=0 protocol_errors=1 timeouts=0 cancelled=0 rejected=0
--- te
HTTP/1.1 501 Not Implemented|Content-Type: text/plain; charset=utf-8|Content-Length: 16|Connection: close||Not Implemented

connections=1 requests=1 handler_errors=0 protocol_errors=1 timeouts=0 cancelled=0 rejected=0
--- continue
HTTP/1.1 100 Continue||HTTP/1.1 200 OK|Content-Type: application/octet-stream|Content-Length: 2||hi
connections=1 requests=1 handler_errors=0 protocol_errors=0 timeouts=0 cancelled=0 rejected=0
--- http10
HTTP/1.1 200 OK|Connection: close||abcdef
connections=1 requests=1 handler_errors=0 protocol_errors=0 timeouts=0 cancelled=0 rejected=0
--- keepalive10
HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 4|Connection: keep-alive||homeHTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 4|Connection: close||home
connections=1 requests=2 handler_errors=0 protocol_errors=0 timeouts=0 cancelled=0 rejected=0
--- unread
HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 0|Connection: close||
connections=1 requests=1 handler_errors=0 protocol_errors=0 timeouts=0 cancelled=0 rejected=0
--- broken
HTTP/1.1 200 OK|Transfer-Encoding: chunked||4|part|
connections=1 requests=1 handler_errors=1 protocol_errors=0 timeouts=0 cancelled=0 rejected=0
--- hang head
HTTP/1.1 408 Request Timeout|Content-Type: text/plain; charset=utf-8|Content-Length: 16|Connection: close||Request Timeout

connections=1 requests=0 handler_errors=0 protocol_errors=0 timeouts=1 cancelled=0 rejected=0
--- hang idle
HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 4||home
connections=1 requests=1 handler_errors=0 protocol_errors=0 timeouts=0 cancelled=0 rejected=0
--- hang body
HTTP/1.1 408 Request Timeout|Content-Type: text/plain; charset=utf-8|Content-Length: 16|Connection: close||Request Timeout

connections=1 requests=1 handler_errors=0 protocol_errors=1 timeouts=1 cancelled=0 rejected=0
--- 405 and 404
HTTP/1.1 405 Method Not Allowed|Allow: POST|Content-Type: text/plain; charset=utf-8|Content-Length: 19||Method Not Allowed
HTTP/1.1 404 Not Found|Content-Type: text/plain; charset=utf-8|Content-Length: 10||Not Found
HTTP/1.1 400 Bad Request|Content-Type: text/plain; charset=utf-8|Content-Length: 12||Bad Request

connections=1 requests=3 handler_errors=0 protocol_errors=0 timeouts=0 cancelled=0 rejected=0
--- max
HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 4||homeHTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 4|Connection: close||home
connections=1 requests=2 handler_errors=0 protocol_errors=0 timeouts=0 cancelled=0 rejected=0
--- body limit
HTTP/1.1 413 Content Too Large|Content-Type: text/plain; charset=utf-8|Content-Length: 18|Connection: close||Content Too Large

connections=1 requests=1 handler_errors=0 protocol_errors=1 timeouts=0 cancelled=0 rejected=0
HTTP/1.1 413 Content Too Large|Content-Type: text/plain; charset=utf-8|Content-Length: 18|Connection: close||Content Too Large

connections=1 requests=1 handler_errors=0 protocol_errors=1 timeouts=0 cancelled=0 rejected=0"##,
    );
}

/// Reading the request body after answering finds no body: both reads
/// report `Broken`, and the handler's success stands.
#[test]
fn reading_after_answering_reports_a_broken_body() {
    runs(
        SCRIPTED,
        r##"
def main() -> Result[(), Failure]:
    config = std.http.server.config()
    scripted("POST /late HTTP/1.1\r\nHost: x\r\nContent-Length: 2\r\n\r\nhi", 64, False, &config)?
    Ok(())
"##,
        r##"HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 4|Connection: close||sent
connections=1 requests=1 handler_errors=0 protocol_errors=0 timeouts=0 cancelled=0 rejected=0"##,
    );
}

/// One worker and one queue slot: while the worker is busy and the slot
/// taken, the next connection is answered `503` on the accepting thread.
#[test]
fn a_full_queue_answers_503() {
    runs(
        LOOPBACK,
        r##"
def client(port: u16, app: &Gated, stop: CancellationToken) -> ():
    mut first = connect_to(port, "GET /wait HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap()
    drop(app.entered.wait_timeout(5000u64))
    mut second = connect_to(port, "GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap()
    std.time.sleep(&std.time.milliseconds(100u32))
    print(fetch(port, "GET / HTTP/1.1\r\nHost: x\r\n\r\n").unwrap()).unwrap()
    app.gate.cancel()
    print(until(&mut first, "").unwrap()).unwrap()
    print(until(&mut second, "").unwrap()).unwrap()
    stop.cancel()

def main() -> Result[(), Failure]:
    table = routes()?
    app = gated()?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    mut config = std.http.server.config()
    config.workers = 1
    config.queue = 1
    with spawn(client, port, &app, stop.share())? as task:
        report = std.http.server.serve(listener, &table, &app, &config, &stop)?
        print(counts(&report)?)?
    Ok(())
"##,
        r##"HTTP/1.1 503 Service Unavailable|Content-Type: text/plain; charset=utf-8|Content-Length: 20|Retry-After: 1|Connection: close||Service Unavailable

HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 6|Connection: close||waited
HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 4|Connection: close||home
connections=2 requests=2 handler_errors=0 protocol_errors=0 timeouts=0 cancelled=0 rejected=1"##,
    );
}

/// After `stop`, an idle keep-alive connection closes at once, a response
/// that watches `stopping()` finishes, and one that ignores it is cut off
/// when the drain deadline cancels its writes.
#[test]
fn shutdown_drains_polite_work_and_cancels_the_rest() {
    runs(
        LOOPBACK,
        r##"
def client(port: u16, path: str, stop: CancellationToken) -> ():
    # An idle keep-alive connection closes at shutdown without waiting.
    mut idle = connect_to(port, "GET / HTTP/1.1\r\nHost: x\r\n\r\n").unwrap()
    drop(until(&mut idle, "home").unwrap())
    mut stream = connect_to(port, &"GET ".concat(&path).unwrap().concat(" HTTP/1.1\r\nHost: x\r\n\r\n").unwrap()).unwrap()
    print(until(&mut stream, "tick\n").unwrap()).unwrap()
    stop.cancel()
    text = until(&mut stream, "").unwrap()
    print(text.endswith("bye\n|0||"), text.endswith("tick\n|")).unwrap()
    print(str.repr(until(&mut idle, "").unwrap()).unwrap()).unwrap()

def run(path: str, drain_ms: u32) -> Result[(), Failure]:
    table = routes()?
    app = gated()?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    mut config = std.http.server.config()
    config.drain_timeout = std.time.milliseconds(drain_ms)
    start = std.time.monotonic_now()
    mut summary = ""
    with spawn(client, port, path, stop.share())? as task:
        report = std.http.server.serve(listener, &table, &app, &config, &stop)?
        summary = counts(&report)?
    print(summary)?
    elapsed = std.time.monotonic_now().saturating_duration_since(&start).as_nanoseconds()
    print(elapsed < 3000000000u64)?
    Ok(())

def main() -> Result[(), Failure]:
    run("/polite", 5000u32)?
    run("/stubborn", 200u32)?
    Ok(())
"##,
        r##"HTTP/1.1 200 OK|Transfer-Encoding: chunked||5|tick

True False
""
connections=2 requests=2 handler_errors=0 protocol_errors=0 timeouts=0 cancelled=0 rejected=0
True
HTTP/1.1 200 OK|Transfer-Encoding: chunked||5|tick

False True
""
connections=2 requests=2 handler_errors=1 protocol_errors=0 timeouts=0 cancelled=1 rejected=0
True"##,
    );
}

/// A half-sent head gets `408` at the head deadline; a connection that
/// sends nothing closes quietly at the idle deadline. Events record both.
#[test]
fn deadlines_close_slow_and_silent_clients() {
    runs(
        LOOPBACK,
        r##"import std.log
from std.log import Event, Value

def client(port: u16, stop: CancellationToken) -> ():
    # Sends half a head and stalls: closed when the head deadline passes.
    mut slow = connect_to(port, "GET / HTTP/1.1\r\nHost: x\r\nHost: x").unwrap()
    # Sends nothing: closed when the idle deadline passes.
    mut silent = connect_to(port, "").unwrap()
    print(str.repr(until(&mut slow, "").unwrap()).unwrap()).unwrap()
    print(str.repr(until(&mut silent, "").unwrap()).unwrap()).unwrap()
    print(fetch(port, "GET /missing HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap()).unwrap()
    stop.cancel()

def main() -> Result[(), Failure]:
    table = routes()?
    app = gated()?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    (sender, receiver) = channel[Event](16u64)?
    mut config = std.http.server.config()
    config.idle_timeout = std.time.milliseconds(200u32)
    config.head_timeout = std.time.milliseconds(200u32)
    config.events = Some(std.log.channel_sink(sender))
    with spawn(client, port, stop.share())? as task:
        report = std.http.server.serve(listener, &table, &app, &config, &stop)?
        print(counts(&report)?)?
    config.events = Nothing
    while True:
        match receiver.recv_nowait():
            case Ok(event):
                mut line = std.log.level_name(&event.level).concat(" ")?.concat(&event.message)?
                for index in range(len(event.fields)):
                    field = &event.fields[index]
                    if field.key != "micros":
                        line = line.concat(" ")?.concat(&field.key)?.concat("=")?.concat(&str.repr(&field.value)?)?
                print(line)?
            case Err(_):
                break
    Ok(())
"##,
        r##""HTTP/1.1 408 Request Timeout|Content-Type: text/plain; charset=utf-8|Content-Length: 16|Connection: close||Request Timeout\n"
""
HTTP/1.1 404 Not Found|Content-Type: text/plain; charset=utf-8|Content-Length: 10|Connection: close||Not Found

connections=3 requests=1 handler_errors=0 protocol_errors=0 timeouts=1 cancelled=0 rejected=0
warn timed out status=std.log.Value.Integer(408)
info request method=std.log.Value.Text("GET") target=std.log.Value.Text("/missing") status=std.log.Value.Integer(404) bytes=std.log.Value.Integer(10)"##,
    );
}

/// Insertion order decides overlapping routes; HEAD falls back to GET; a
/// wrong method lists the allowed ones; a bad configuration is refused.
#[test]
fn routes_match_in_order_and_patterns_are_checked() {
    runs(
        "",
        r##"import std.http.server
import std.net
from std.http.server import Match, Router, ServerError
from std.sync import CancellationToken

def shown(table: &Router, method: &str, path: &str) -> Result[(), Failure]:
    print(method, path, str.repr(&table.find(method, path))?)?
    Ok(())

pub class Nobody:
    pub def handle(self, exchange: &mut std.http.server.Exchange[std.net.TcpStream]) -> Result[(), Failure]:
        Ok(())

def main() -> Result[(), Failure]:
    mut table = std.http.server.router()?
    print(str.repr(&table.add("GET", "/users/{id}", 1))?)?
    print(str.repr(&table.add("GET", "/users/{other}", 2))?)?
    print(str.repr(&table.add("PUT", "/users/{id}", 3))?)?
    print(str.repr(&table.add("GET", "/users/me", 4))?)?
    print(str.repr(&table.add("GET", "/static/{path...}", 5))?)?
    print(str.repr(&table.add("GET", "/a/{rest...}/b", 6))?)?
    print(str.repr(&table.add("GET", "users", 6))?)?
    print(str.repr(&table.add("G T", "/x", 6))?)?
    print(str.repr(&table.add("GET", "/{}", 6))?)?
    print(str.repr(&table.add("GET", "/x{y}", 6))?)?
    print(str.repr(&table.add("GET", "/", 7))?)?
    shown(&table, "GET", "/users/42")?
    shown(&table, "GET", "/users/me")?
    shown(&table, "HEAD", "/users/42")?
    shown(&table, "DELETE", "/users/42")?
    shown(&table, "GET", "/users/")?
    shown(&table, "GET", "/users/42/")?
    shown(&table, "GET", "/static")?
    shown(&table, "GET", "/static/")?
    shown(&table, "GET", "/static/a/b.css")?
    shown(&table, "GET", "/")?
    shown(&table, "GET", "/nothing")?
    mut config = std.http.server.config()
    config.workers = 0
    listener = std.net.listen(&std.net.loopback_v4(0u16), 4)?
    stop = CancellationToken()?
    print(str.repr(&std.http.server.serve(listener, &table, &Nobody(), &config, &stop))?)?
    Ok(())
"##,
        r##"Result[(), std.http.server.ServerError].Ok(())
Result[(), std.http.server.ServerError].Err(std.http.server.ServerError.DuplicateRoute)
Result[(), std.http.server.ServerError].Ok(())
Result[(), std.http.server.ServerError].Ok(())
Result[(), std.http.server.ServerError].Ok(())
Result[(), std.http.server.ServerError].Err(std.http.server.ServerError.InvalidPattern)
Result[(), std.http.server.ServerError].Err(std.http.server.ServerError.InvalidPattern)
Result[(), std.http.server.ServerError].Err(std.http.server.ServerError.InvalidPattern)
Result[(), std.http.server.ServerError].Err(std.http.server.ServerError.InvalidPattern)
Result[(), std.http.server.ServerError].Err(std.http.server.ServerError.InvalidPattern)
Result[(), std.http.server.ServerError].Ok(())
GET /users/42 Result[std.http.server.Match, std.http.server.ServerError].Ok(std.http.server.Match.Found(1, ["42"]))
GET /users/me Result[std.http.server.Match, std.http.server.ServerError].Ok(std.http.server.Match.Found(1, ["me"]))
HEAD /users/42 Result[std.http.server.Match, std.http.server.ServerError].Ok(std.http.server.Match.Found(1, ["42"]))
DELETE /users/42 Result[std.http.server.Match, std.http.server.ServerError].Ok(std.http.server.Match.MethodNotAllowed("GET, PUT, HEAD"))
GET /users/ Result[std.http.server.Match, std.http.server.ServerError].Ok(std.http.server.Match.NotFound)
GET /users/42/ Result[std.http.server.Match, std.http.server.ServerError].Ok(std.http.server.Match.NotFound)
GET /static Result[std.http.server.Match, std.http.server.ServerError].Ok(std.http.server.Match.Found(5, [""]))
GET /static/ Result[std.http.server.Match, std.http.server.ServerError].Ok(std.http.server.Match.Found(5, [""]))
GET /static/a/b.css Result[std.http.server.Match, std.http.server.ServerError].Ok(std.http.server.Match.Found(5, ["a/b.css"]))
GET / Result[std.http.server.Match, std.http.server.ServerError].Ok(std.http.server.Match.Found(7, []))
GET /nothing Result[std.http.server.Match, std.http.server.ServerError].Ok(std.http.server.Match.NotFound)
Result[std.http.server.Report, std.http.server.ServerError].Err(std.http.server.ServerError.InvalidConfig)"##,
    );
}

/// A worker that cannot start fails `serve`; workers already started see
/// their queue disconnect and are joined before it returns.
#[cfg(feature = "runtime-checks")]
#[test]
fn a_worker_that_cannot_start_fails_serve() {
    runs(
        LOOPBACK,
        r##"
def main() -> Result[(), Failure]:
    table = routes()?
    app = gated()?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 4)?
    stop = CancellationToken()?
    mut config = std.http.server.config()
    config.workers = 3
    print("__test_one_thread_start__")?
    outcome = std.http.server.serve(listener, &table, &app, &config, &stop)
    print("__test_restore_thread_starts__")?
    print(str.repr(&outcome)?)?
    Ok(())
"##,
        "Result[std.http.server.Report, std.http.server.ServerError].Err(std.http.server.ServerError.Thread(ThreadError.System(11)))",
    );
}

/// Allocation failure at any point closes or answers the connection; the
/// program never aborts, and once allocation succeeds the request is served.
#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_closes_the_connection_cleanly() {
    let attempts = 150;
    let main = format!(
        r##"
def sample() -> Result[Bytes, Failure]:
    Ok(std.bytes.from_text("POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\nX-Long-Field-Name-Beyond-Inline: a-value-beyond-inline-storage\r\n\r\n3\r\nabc\r\n0\r\n\r\nGET /hello/a-name-beyond-inline HTTP/1.1\r\nHost: x\r\n\r\n")?)

def main() -> Result[(), Failure]:
    table = routes()?
    app = App("home")
    config = std.http.server.config()
    stop = CancellationToken()?
    for attempt in range({attempts}):
        input = sample()?
        marker = "__test_fail_allocations_after_".concat(str.from(attempt)?)?.concat("__")?
        print(marker)?
        report = std.http.server.serve_connection(Script(input, 0, 7, False, False), &table, &app, &config, &stop, &stop)
        print("__test_restore_allocations__")?
        print(report.requests)?
    Ok(())
"##
    );
    let result = output(&format!("{SCRIPTED}{main}"));
    let served: Vec<&str> = result.lines().collect();
    assert_eq!(served.len(), attempts);
    assert!(served.iter().all(|count| ["0", "1", "2"].contains(count)));
    assert_eq!(served[0], "0");
    assert!(
        served[attempts - 20..].iter().all(|count| *count == "2"),
        "{served:?}"
    );
}

/// One worker serves ten keep-alive connections in turn: idle connections
/// wait on the accepting thread's selector, not on the worker. Shutdown
/// closes them at once. With `idle_connections` full, a new connection gets
/// `503`; a parked one still closes at its idle deadline.
#[test]
fn idle_connections_wait_without_a_worker() {
    runs(
        LOOPBACK,
        r##"
def ask(stream: &mut TcpStream) -> ():
    never = CancellationToken().unwrap()
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(5u32)).unwrap()
    request = std.bytes.from_text("GET / HTTP/1.1\r\nHost: x\r\n\r\n").unwrap()
    stream.write_all_before(&request, 0, request.len(), &deadline, &never).unwrap()

def many(port: u16, stop: CancellationToken) -> ():
    mut streams = list[TcpStream].new().unwrap()
    for index in range(10):
        mut stream = connect_to(port, "GET / HTTP/1.1\r\nHost: x\r\n\r\n").unwrap()
        drop(until(&mut stream, "home").unwrap())
        streams.append(stream).unwrap()
    mut answered = 0
    for round in range(3):
        for index in range(10):
            ask(&mut streams[index])
        for index in range(10):
            if until(&mut streams[index], "home").unwrap().endswith("home"):
                answered += 1
    print(answered).unwrap()
    stop.cancel()
    mut closed = 0
    for index in range(10):
        if until(&mut streams[index], "").unwrap() == "":
            closed += 1
    print(closed).unwrap()

def capped(port: u16, stop: CancellationToken) -> ():
    mut first = connect_to(port, "").unwrap()
    mut second = connect_to(port, "").unwrap()
    std.time.sleep(&std.time.milliseconds(50u32))
    print(fetch(port, "").unwrap()).unwrap()
    ask(&mut first)
    print(until(&mut first, "home").unwrap()).unwrap()
    # Parked again after its request, then closed at the idle deadline.
    print(str.repr(until(&mut first, "").unwrap()).unwrap()).unwrap()
    print(str.repr(until(&mut second, "").unwrap()).unwrap()).unwrap()
    stop.cancel()

def main() -> Result[(), Failure]:
    table = routes()?
    app = gated()?
    mut config = std.http.server.config()
    config.workers = 1
    config.queue = 16
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    # The client prints after it sees each connection close, which can be
    # after `serve` returns, so the report is printed once the client joins.
    mut first: Option[Report] = Nothing
    with spawn(many, port, stop.share())? as task:
        first = Some(std.http.server.serve(listener, &table, &app, &config, &stop)?)
    print(counts(&first.unwrap())?)?
    config.idle_connections = 2
    config.idle_timeout = std.time.milliseconds(300u32)
    second = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    other = second.local_address()?.port
    halt = CancellationToken()?
    with spawn(capped, other, halt.share())? as task:
        report = std.http.server.serve(second, &table, &app, &config, &halt)?
        print(counts(&report)?)?
    Ok(())
"##,
        r##"30
10
connections=10 requests=40 handler_errors=0 protocol_errors=0 timeouts=0 cancelled=0 rejected=0
HTTP/1.1 503 Service Unavailable|Content-Type: text/plain; charset=utf-8|Content-Length: 20|Retry-After: 1|Connection: close||Service Unavailable

HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 4||home
""
""
connections=2 requests=1 handler_errors=0 protocol_errors=0 timeouts=0 cancelled=0 rejected=1"##,
    );
}

/// A request head that arrives in pieces is gathered on the accepting
/// thread, so three partial heads leave the only worker free: a head that
/// completes is served, one that reaches `max_head` is refused at once, and
/// one that stalls gets `408` at its head deadline. With `idle_connections`
/// full, a pipelined partial head that a worker hands back is parked anyway
/// and answered at its deadline, while a new connection is turned away.
#[test]
fn partial_heads_wait_without_a_worker() {
    let main = r##"
def send(stream: &mut TcpStream, raw: &str) -> ():
    never = CancellationToken().unwrap()
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(5u32)).unwrap()
    request = std.bytes.from_text(raw).unwrap()
    stream.write_all_before(&request, 0, request.len(), &deadline, &never).unwrap()

def millis_since(start: &std.time.Instant) -> u64:
    std.time.monotonic_now().saturating_duration_since(start).as_nanoseconds() // 1000000u64

def pieces(port: u16, stop: CancellationToken) -> ():
    start = std.time.monotonic_now()
    mut stalled = connect_to(port, "GET / HTTP/1.1\r\nHost: x").unwrap()
    mut split = connect_to(port, "GET / HTTP/1.1\r\nHo").unwrap()
    mut large = connect_to(port, "GET / HTTP/1.1\r\nHost: x\r\nX-Long: ").unwrap()
    std.time.sleep(&std.time.milliseconds(100u32))
    print(fetch(port, "GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap()).unwrap()
    send(&mut split, "st: x\r\nConnection: close\r\n\r\n")
    print(until(&mut split, "").unwrap()).unwrap()
    # Brings the head to exactly `max_head` bytes with no end in sight.
    send(&mut large, "FILLER")
    print(until(&mut large, "").unwrap()).unwrap()
    print(millis_since(&start) < 800u64).unwrap()
    print(until(&mut stalled, "").unwrap()).unwrap()
    print(millis_since(&start) >= 1000u64).unwrap()
    stop.cancel()

def crowded(port: u16, app: &Gated, stop: CancellationToken) -> ():
    # Takes the worker, then leaves a partial head behind its request.
    mut first = connect_to(port, "GET /wait HTTP/1.1\r\nHost: x\r\n\r\nGET / HT").unwrap()
    drop(app.entered.wait_timeout(5000u64))
    # Takes the only parking slot.
    mut silent = connect_to(port, "").unwrap()
    std.time.sleep(&std.time.milliseconds(100u32))
    app.gate.cancel()
    drop(until(&mut first, "waited").unwrap())
    std.time.sleep(&std.time.milliseconds(50u32))
    print(fetch(port, "GET / HTTP/1.1\r\nHost: x\r\n\r\n").unwrap()).unwrap()
    print(until(&mut first, "").unwrap()).unwrap()
    print(str.repr(until(&mut silent, "").unwrap()).unwrap()).unwrap()
    stop.cancel()

def main() -> Result[(), Failure]:
    table = routes()?
    app = gated()?
    mut config = std.http.server.config()
    config.workers = 1
    config.head_timeout = std.time.milliseconds(1000u32)
    config.limits.max_head = 256
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    with spawn(pieces, port, stop.share())? as task:
        report = std.http.server.serve(listener, &table, &app, &config, &stop)?
        print(counts(&report)?)?
    config.idle_connections = 1
    config.idle_timeout = std.time.milliseconds(300u32)
    second = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    other = second.local_address()?.port
    halt = CancellationToken()?
    with spawn(crowded, other, &app, halt.share())? as task:
        report = std.http.server.serve(second, &table, &app, &config, &halt)?
        print(counts(&report)?)?
    Ok(())
"##
    .replace("FILLER", &"a".repeat(256 - 33));
    runs(
        LOOPBACK,
        &main,
        r##"HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 4|Connection: close||home
HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 4|Connection: close||home
HTTP/1.1 431 Request Header Fields Too Large|Content-Type: text/plain; charset=utf-8|Content-Length: 32|Connection: close||Request Header Fields Too Large

True
HTTP/1.1 408 Request Timeout|Content-Type: text/plain; charset=utf-8|Content-Length: 16|Connection: close||Request Timeout

True
connections=4 requests=2 handler_errors=0 protocol_errors=1 timeouts=1 cancelled=0 rejected=0
HTTP/1.1 503 Service Unavailable|Content-Type: text/plain; charset=utf-8|Content-Length: 20|Retry-After: 1|Connection: close||Service Unavailable

HTTP/1.1 408 Request Timeout|Content-Type: text/plain; charset=utf-8|Content-Length: 16|Connection: close||Request Timeout

""
connections=2 requests=1 handler_errors=0 protocol_errors=0 timeouts=1 cancelled=0 rejected=1"##,
    );
}

fn fixtures() -> String {
    format!("{}/tests/fixtures/tls", env!("CARGO_MANIFEST_DIR"))
}

/// HTTPS through `std.http.tls.serve`. Two TLS clients alternate on one
/// worker, so an idle TLS connection is parked too. A client that never
/// starts its handshake is closed at `handshake_timeout` and one that sends
/// plaintext fails its handshake; both are counted and logged.
#[test]
fn https_serves_tls_connections_under_a_handshake_deadline() {
    let main = r##"import std.http.tls
import std.log
import std.tls
from std.http.tls import TlsTransport
from std.log import Event
from std.tls import TlsStream

pub class Secure:
    pub def handle(self, exchange: &mut Exchange[TlsTransport]) -> Result[(), Failure]:
        exchange.send_text(200, "secure")?
        Ok(())

def ask(stream: &mut TlsStream) -> str:
    never = CancellationToken().unwrap()
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(5u32)).unwrap()
    request = std.bytes.from_text("GET / HTTP/1.1\r\nHost: x\r\n\r\n").unwrap()
    stream.write_all_before(&request, 0, request.len(), &deadline, &never).unwrap()
    # Responses are length-delimited; the server sends no close_notify.
    mut out = std.bytes.empty().unwrap()
    while not out.to_text().unwrap().endswith("secure"):
        at = out.len()
        out.resize(at + 64, 0u8).unwrap()
        count = stream.read_before(&mut out, at, at + 64, &deadline, &never).unwrap()
        out.truncate(at + count).unwrap()
    out.to_text().unwrap().replace("\r\n", "|").unwrap()

def secure(port: u16) -> TlsStream:
    never = CancellationToken().unwrap()
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(5u32)).unwrap()
    roots = std.tls.roots_from_pem(&read_text("FIXTURES/ca.pem").unwrap()).unwrap()
    config = std.tls.client_config(&roots).unwrap()
    tcp = std.net.connect(&std.net.loopback_v4(port), &deadline, &never).unwrap()
    std.tls.connect(tcp, &config, "localhost", &deadline, &never).unwrap()

def closes(stream: &mut TcpStream) -> bool:
    never = CancellationToken().unwrap()
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(5u32)).unwrap()
    mut buffer = std.bytes.with_capacity(256).unwrap()
    buffer.resize(256, 0u8).unwrap()
    while True:
        match stream.read_before(&mut buffer, 0, 256, &deadline, &never):
            case Ok(0):
                return True
            case Ok(_):
                pass
            case Err(NetError.TimedOut):
                return False
            case Err(_):
                return True
    False

def client(port: u16, stop: CancellationToken) -> ():
    mut first = secure(port)
    mut second = secure(port)
    for round in range(2):
        print(ask(&mut first)).unwrap()
        print(ask(&mut second)).unwrap()
    mut silent = connect_to(port, "").unwrap()
    print(closes(&mut silent)).unwrap()
    mut plain = connect_to(port, "GET / HTTP/1.1\r\nHost: x\r\n\r\n").unwrap()
    print(closes(&mut plain)).unwrap()
    stop.cancel()

def main() -> Result[(), Failure]:
    table = routes()?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    tls = std.tls.server_config(&read_text("FIXTURES/leaf.pem")?, &read_text("FIXTURES/leaf.key")?).unwrap()
    (sender, receiver) = channel[Event](16u64)?
    mut config = std.http.server.config()
    config.workers = 1
    config.handshake_timeout = std.time.milliseconds(300u32)
    config.events = Some(std.log.channel_sink(sender))
    with spawn(client, port, stop.share())? as task:
        report = std.http.tls.serve(listener, tls, &table, &Secure(), &config, &stop)?
        print(counts(&report)?)?
    config.events = Nothing
    while True:
        match receiver.recv_nowait():
            case Ok(event):
                if event.message != "request":
                    print(event.message)?
            case Err(_):
                break
    Ok(())
"##
    .replace("FIXTURES", &fixtures());
    runs(
        LOOPBACK,
        &main,
        r##"HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 6||secure
HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 6||secure
HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 6||secure
HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 6||secure
True
True
connections=4 requests=4 handler_errors=0 protocol_errors=1 timeouts=1 cancelled=0 rejected=0
handshake timed out
handshake failed"##,
    );
}

/// HTTPS connections do not hold the only worker while they wait for their
/// peer: one that sends part of a ClientHello waits parked until
/// `handshake_timeout`, and one that sends part of a head after its
/// handshake waits parked until its `408`, while two other clients are
/// served in turn.
#[test]
fn https_partial_handshakes_and_heads_wait_without_a_worker() {
    let main = r##"import std.http.tls
import std.log
import std.tls
from std.http.tls import TlsTransport
from std.log import Event
from std.tls import TlsStream

pub class Secure:
    pub def handle(self, exchange: &mut Exchange[TlsTransport]) -> Result[(), Failure]:
        exchange.send_text(200, "secure")?
        Ok(())

# Sends `raw` and reads until the answer ends with `marker`; the server
# sends no close_notify, so the end of a closed connection is an error.
def ask(stream: &mut TlsStream, raw: &str, marker: &str) -> str:
    never = CancellationToken().unwrap()
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(5u32)).unwrap()
    request = std.bytes.from_text(raw).unwrap()
    stream.write_all_before(&request, 0, request.len(), &deadline, &never).unwrap()
    mut out = std.bytes.empty().unwrap()
    while marker.byte_len() > 0 and not out.to_text().unwrap().endswith(marker):
        at = out.len()
        out.resize(at + 64, 0u8).unwrap()
        count = stream.read_before(&mut out, at, at + 64, &deadline, &never).unwrap()
        out.truncate(at + count).unwrap()
    out.to_text().unwrap().replace("\r\n", "|").unwrap()

def secure(port: u16) -> TlsStream:
    never = CancellationToken().unwrap()
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(5u32)).unwrap()
    roots = std.tls.roots_from_pem(&read_text("FIXTURES/ca.pem").unwrap()).unwrap()
    config = std.tls.client_config(&roots).unwrap()
    tcp = std.net.connect(&std.net.loopback_v4(port), &deadline, &never).unwrap()
    std.tls.connect(tcp, &config, "localhost", &deadline, &never).unwrap()

def closes(stream: &mut TcpStream) -> bool:
    never = CancellationToken().unwrap()
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(5u32)).unwrap()
    mut buffer = std.bytes.with_capacity(256).unwrap()
    buffer.resize(256, 0u8).unwrap()
    while True:
        match stream.read_before(&mut buffer, 0, 256, &deadline, &never):
            case Ok(0):
                return True
            case Ok(_):
                pass
            case Err(NetError.TimedOut):
                return False
            case Err(_):
                return True
    False

def client(port: u16, stop: CancellationToken) -> ():
    start = std.time.monotonic_now()
    # A TLS record header promising 512 bytes of handshake, then one byte.
    mut hello = std.net.connect(&std.net.loopback_v4(port), &std.time.deadline_after(&start, &std.time.seconds(5u32)).unwrap(), &CancellationToken().unwrap()).unwrap()
    partial = std.bytes.from_list(&[22u8, 3u8, 1u8, 2u8, 0u8, 1u8].unwrap()).unwrap()
    hello.write_all_before(&partial, 0, partial.len(), &std.time.deadline_after(&start, &std.time.seconds(5u32)).unwrap(), &CancellationToken().unwrap()).unwrap()
    mut stalled = secure(port)
    drop(ask(&mut stalled, "GET / HTTP/1.1\r\nHo", ""))
    mut first = secure(port)
    mut second = secure(port)
    for round in range(2):
        print(ask(&mut first, "GET / HTTP/1.1\r\nHost: x\r\n\r\n", "secure")).unwrap()
        print(ask(&mut second, "GET / HTTP/1.1\r\nHost: x\r\n\r\n", "secure")).unwrap()
    waited = std.time.monotonic_now().saturating_duration_since(&start).as_nanoseconds() // 1000000u64
    print(waited < 700u64).unwrap()
    print(ask(&mut stalled, "", "Request Timeout\n")).unwrap()
    print(closes(&mut hello)).unwrap()
    stop.cancel()

def main() -> Result[(), Failure]:
    table = routes()?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    tls = std.tls.server_config(&read_text("FIXTURES/leaf.pem")?, &read_text("FIXTURES/leaf.key")?).unwrap()
    (sender, receiver) = channel[Event](16u64)?
    mut config = std.http.server.config()
    config.workers = 1
    config.handshake_timeout = std.time.milliseconds(1000u32)
    # Later than the handshake deadline, so the events come in a fixed order.
    config.head_timeout = std.time.milliseconds(1500u32)
    config.events = Some(std.log.channel_sink(sender))
    with spawn(client, port, stop.share())? as task:
        report = std.http.tls.serve(listener, tls, &table, &Secure(), &config, &stop)?
        print(counts(&report)?)?
    config.events = Nothing
    while True:
        match receiver.recv_nowait():
            case Ok(event):
                if event.message != "request":
                    print(event.message)?
            case Err(_):
                break
    Ok(())
"##
    .replace("FIXTURES", &fixtures());
    runs(
        LOOPBACK,
        &main,
        r##"HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 6||secure
HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 6||secure
HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 6||secure
HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 6||secure
True
HTTP/1.1 408 Request Timeout|Content-Type: text/plain; charset=utf-8|Content-Length: 16|Connection: close||Request Timeout

True
connections=4 requests=4 handler_errors=0 protocol_errors=0 timeouts=2 cancelled=0 rejected=0
handshake timed out
timed out"##,
    );
}

/// Issue #135: routes and captures share the method, pattern and path text
/// instead of copying it, so long strings need no more allocations than
/// short ones.
#[cfg(feature = "runtime-checks")]
#[test]
fn routes_and_captures_share_their_strings() {
    runs(
        "",
        r##"import std.http.server
from std.http.server import Match, Router, ServerError

def attempt(table: &mut Router, method: &str, pattern: &str, path: &str) -> Result[Match, ServerError]:
    table.add(method, pattern, 1)?
    table.find(method, path)

def needed(method: &str, pattern: &str, path: &str) -> Result[i64, Failure]:
    for budget in range(200):
        mut table = std.http.server.router()?
        marker = "__test_fail_allocations_after_".concat(str.from(budget)?)?.concat("__")?
        print(marker)?
        outcome = attempt(&mut table, method, pattern, path)
        print("__test_restore_allocations__")?
        if outcome.is_ok():
            return Ok(budget)
    Ok(-1)

def main() -> Result[(), Failure]:
    base = needed("GET", "/f/{name}", "/f/a")?
    print(needed("PROPFIND-BEYOND-INLINE", "/f/{name}", "/f/a")? - base)?
    print(needed("GET", "/f/{name}", "/f/a-name-beyond-inline")? - base)?
    print(needed("GET", "/files-beyond-inline/{name}", "/files-beyond-inline/a")? - base)?
    Ok(())
"##,
        // A long method costs nothing extra; a long path or pattern piece
        // costs only the allocation `split` makes for it.
        "0\n1\n2",
    );
}
