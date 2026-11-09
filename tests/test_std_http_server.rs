//! `std.http.server`: routing, workers, admission, deadlines and shutdown.
//! Scripted connections run in memory through `serve_connection`; the rest
//! use loopback sockets on ephemeral ports. No test needs a network.
mod support;
#[cfg(feature = "runtime-checks")]
use support::http_program_output as output;
use support::tls_fixtures as fixtures;

fn runs(prelude: &str, main: &str, expected: &str) {
    assert_eq!(
        support::without_http_dates(&support::http_program_output(&format!("{prelude}{main}"))),
        expected.trim_end()
    );
}

/// An in-memory `Transport` and a handler covering each response style.
const SCRIPTED: &str = r##"import std.bytes
import std.http
import std.http.server
import std.io
import std.net
import std.time
from std.bytes import Bytes
from std.http import Header, HttpError
from std.http.server import Config, Exchange, Report, Router, ServerError, Transport
from std.net import NetError, Shutdown, TcpStream
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
        # at is a cursor in the scripted input; destination and step bound
        # each count, so at + count never exceeds input.len().
        unchecked:
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
            buffer.overwrite(start, &self.input[self.at:self.at + count])
            self.at += count
            Ok(count)

    pub def shutdown(self: &mut Script, direction: Shutdown) -> Result[(), NetError]:
        Ok(())

    pub def write_before(self: &mut Script, source: &[u8], deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, NetError]:
        if cancel.is_cancelled():
            return Err(NetError.Cancelled)
        if self.echo:
            mut out = std.io.stdout()
            out.write(source).unwrap()
        Ok(len(source))

pub enum Route:
    Home
    Param
    Echo
    Fail
    Stream
    Silent
    Sized
    Query
    Broken
    Authority
    Late
    Declared

# The deliberate errors routes return, so a test can see how each is answered.
pub enum Deliberate:
    Fail
    Unexpected

pub class App:
    pub label: str

    pub def handle(self, route: &Route, exchange: &mut Exchange[Script]) -> Result[(), std.bytes.BytesError | Deliberate | std.http.HttpError | AllocError]:
        answer(&self.label, route, exchange)

pub class NetApp:
    pub label: str

    pub def handle(self, route: &Route, exchange: &mut Exchange[TcpStream]) -> Result[(), std.bytes.BytesError | Deliberate | std.http.HttpError | AllocError]:
        answer(&self.label, route, exchange)

pub def answer[T: Transport](label: &str, route: &Route, exchange: &mut Exchange[T]) -> Result[(), std.bytes.BytesError | Deliberate | std.http.HttpError | AllocError]:
        match route:
            case Route.Home:
                exchange.send_text(200, label)?
            case Route.Param:
                param = exchange.params[0].concat("|")?
                exchange.send_text(200, &param)?
            case Route.Echo:
                mut body = std.bytes.empty()?
                exchange.read_all(&mut body)?
                exchange.send(200, "application/octet-stream", &body)?
            case Route.Fail:
                return Err(Deliberate.Fail)
            case Route.Stream:
                exchange.start(std.http.response(200, "OK")?)?
                piece = std.bytes.from_text("abcdef")?
                exchange.write(&piece[:2])?
                exchange.write(&piece[2:6])?
            case Route.Silent:
                pass
            case Route.Sized:
                mut head = std.http.response(200, "OK")?
                head.add("Content-Length", "5")?
                exchange.start(head)?
                piece = std.bytes.from_text("hello")?
                exchange.write(&piece[:5])?
                exchange.end()?
            case Route.Query:
                query = exchange.query.concat("")?
                exchange.send_text(200, &query)?
            case Route.Broken:
                exchange.start(std.http.response(200, "OK")?)?
                piece = std.bytes.from_text("part")?
                exchange.write(&piece[:4])?
                return Err(Deliberate.Fail)
            case Route.Authority:
                authority = exchange.authority.concat("|")?
                exchange.send_text(200, &authority)?
            case Route.Late:
                # Answering gives the connection to the response, so the body
                # is gone; reads report `Broken` without choosing a refusal.
                exchange.send_text(200, "sent")?
                mut buffer = std.bytes.from_text("....")?
                mut out = std.bytes.empty()?
                match exchange.read_body(&mut buffer, 0, 4):
                    case Err(HttpError.Broken):
                        pass
                    case _:
                        return Err(Deliberate.Unexpected)
                match exchange.read_all(&mut out):
                    case Err(HttpError.Broken):
                        pass
                    case _:
                        return Err(Deliberate.Unexpected)
            case Route.Declared:
                # Declares a length but writes nothing, as a HEAD answer may.
                mut head = std.http.response(200, "OK")?
                head.add("Content-Length", "5")?
                exchange.start(head)?
                exchange.end()?
        Ok(())

pub def routes() -> Result[Router[Route], ServerError]:
    mut table = std.http.server.router[Route]()?
    table.add("GET", "/", Route.Home)?
    table.add("GET", "/hello/{name}", Route.Param)?
    table.add("POST", "/echo", Route.Echo)?
    table.add("GET", "/fail", Route.Fail)?
    table.add("GET", "/stream", Route.Stream)?
    table.add("GET", "/silent", Route.Silent)?
    table.add("GET", "/sized", Route.Sized)?
    table.add("*", "/query", Route.Query)?
    table.add("GET", "/broken", Route.Broken)?
    table.add("GET", "/files/{path...}", Route.Param)?
    table.add("GET", "/authority", Route.Authority)?
    table.add("POST", "/late", Route.Late)?
    table.add("GET", "/declared", Route.Declared)?
    Ok(table)

pub def counts(report: &Report) -> Result[str, AllocError]:
    mut text = "connections=".concat(str.from(report.connections)?)?
    text = text.concat(" requests=")?.concat(str.from(report.requests)?)?
    text = text.concat(" handler_errors=")?.concat(str.from(report.handler_errors)?)?
    text = text.concat(" protocol_errors=")?.concat(str.from(report.protocol_errors)?)?
    text = text.concat(" timeouts=")?.concat(str.from(report.timeouts)?)?
    text = text.concat(" cancelled=")?.concat(str.from(report.cancelled)?)?
    text = text.concat(" rejected=")?.concat(str.from(report.rejected)?)?
    Ok(text)

# Serves one scripted connection and prints its counts.
pub def scripted(input: &str, step: i64, hang: bool, config: &Config) -> Result[(), std.bytes.BytesError | std.http.server.ServerError | AllocError | IoError]:
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
pub enum Route:
    Home
    Wait
    Polite
    Stubborn

pub class Gated:
    pub entered: CancellationToken
    pub gate: CancellationToken

    pub def handle(self, route: &Route, exchange: &mut Exchange[TcpStream]) -> Result[(), AllocError | std.bytes.BytesError | std.http.HttpError]:
        match route:
            case Route.Home:
                exchange.send_text(200, "home")?
            case Route.Wait:
                self.entered.cancel()
                drop(self.gate.wait_timeout(5000u64))
                exchange.send_text(200, "waited")?
            case Route.Polite:
                # Streams until shutdown begins, then finishes politely.
                exchange.start(std.http.response(200, "OK")?)?
                tick = std.bytes.from_text("tick\n")?
                while not exchange.stopping():
                    exchange.write(&tick)?
                    std.time.sleep(&std.time.milliseconds(20u32))
                bye = std.bytes.from_text("bye\n")?
                exchange.write(&bye)?
            case Route.Stubborn:
                # Ignores shutdown; only cancellation stops it.
                exchange.start(std.http.response(200, "OK")?)?
                tick = std.bytes.from_text("tick\n")?
                while True:
                    exchange.write(&tick)?
                    std.time.sleep(&std.time.milliseconds(20u32))
        Ok(())

pub def gated() -> Result[Gated, AllocError]:
    Ok(Gated(CancellationToken()?, CancellationToken()?))

pub def routes() -> Result[std.http.server.Router[Route], std.http.server.ServerError]:
    mut table = std.http.server.router[Route]()?
    table.add("GET", "/", Route.Home)?
    table.add("GET", "/wait", Route.Wait)?
    table.add("GET", "/polite", Route.Polite)?
    table.add("GET", "/stubborn", Route.Stubborn)?
    Ok(table)

pub def connect_to(port: u16, raw: &str) -> Result[TcpStream, NetError]:
    never = CancellationToken().unwrap()
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(5u32)).unwrap()
    mut stream = std.net.connect(&std.net.loopback_v4(port), &deadline, &never)?
    request = std.bytes.from_text(raw).unwrap()
    stream.write_all_before(&request, &deadline, &never)?
    Ok(stream)

# Reads until `marker` has arrived, or to the end when it is empty.
pub def until(stream: &mut TcpStream, marker: &str) -> Result[str, NetError]:
    never = CancellationToken().unwrap()
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(5u32)).unwrap()
    mut out = std.bytes.empty().unwrap()
    while True:
        at = out.len()
        extent = checked:
            at + 1
        end = extent.unwrap()
        out.resize(end, 0u8).unwrap()
        count = stream.read_before(&mut out, at, end, &deadline, &never)?
        consumed = checked:
            at + count
        out.truncate(consumed.unwrap())
        if count == 0:
            break
        if marker.byte_len() > 0 and out.to_text().unwrap().endswith(marker):
            break
    Ok(out.to_text().unwrap().replace("\r\n", "|").unwrap())

pub def fetch(port: u16, raw: &str) -> Result[str, NetError]:
    mut stream = connect_to(port, raw)?
    until(&mut stream, "")

pub def counts(report: &Report) -> Result[str, AllocError]:
    mut text = "connections=".concat(str.from(report.connections)?)?
    text = text.concat(" requests=")?.concat(str.from(report.requests)?)?
    text = text.concat(" handler_errors=")?.concat(str.from(report.handler_errors)?)?
    text = text.concat(" protocol_errors=")?.concat(str.from(report.protocol_errors)?)?
    text = text.concat(" timeouts=")?.concat(str.from(report.timeouts)?)?
    text = text.concat(" cancelled=")?.concat(str.from(report.cancelled)?)?
    text = text.concat(" rejected=")?.concat(str.from(report.rejected)?)?
    Ok(text)
"##;

/// Ten pipelined requests on one connection, delivered five bytes at a time:
/// literal, captured and rest routes, query text, a chunked request body,
/// HEAD, chunked and sized streams, and a handler that never answers.
#[test]
fn scripted_requests_are_routed_framed_and_pipelined() {
    runs(
        SCRIPTED,
        r##"
def main() -> Result[(), std.bytes.BytesError | std.http.server.ServerError | AllocError | IoError]:
    config = std.http.server.config()
    scripted("GET / HTTP/1.1\r\nHost: x\r\n\r\nGET /hello/a%2Fb HTTP/1.1\r\nHost: x\r\n\r\nGET /files/x/y/z HTTP/1.1\r\nHost: x\r\n\r\nGET /files/ HTTP/1.1\r\nHost: x\r\n\r\nGET /query?a=1&b HTTP/1.1\r\nHost: x\r\n\r\nPOST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n0\r\n\r\nHEAD /stream HTTP/1.1\r\nHost: x\r\n\r\nGET /stream HTTP/1.1\r\nHost: x\r\n\r\nGET /sized HTTP/1.1\r\nHost: x\r\n\r\nGET /silent HTTP/1.1\r\nHost: x\r\n\r\n", 5, False, &config)?
    Ok(())
"##,
        r##"HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 4||homeHTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 4||a/b|HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 6||x/y/z|HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 1|||HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 5||a=1&bHTTP/1.1 200 OK|Content-Type: application/octet-stream|Content-Length: 3||abcHTTP/1.1 200 OK|Transfer-Encoding: chunked||HTTP/1.1 200 OK|Transfer-Encoding: chunked||2|ab|4|cdef|0||HTTP/1.1 200 OK|Content-Length: 5||helloHTTP/1.1 500 Internal Server Error|Content-Type: text/plain; charset=utf-8|Content-Length: 22||Internal Server Error

connections=1 requests=10 handler_errors=1 protocol_errors=0 timeouts=0 cancelled=0 rejected=0"##,
    );
}

/// `reason` covers every permanent IANA registration (issue #785). Every other
/// value from -1 to 999 gives `""`: unregistered codes, "(Unused)" 306 and
/// 418, and the provisional 104.
#[test]
fn reason_phrases_follow_the_iana_registry() {
    runs(
        "import std.http.server\n",
        r##"
def main() -> Result[(), IoError]:
    for status in range(-1, 1000):
        text = std.http.server.reason(status)
        if text.byte_len() > 0:
            print(status, text)?
    Ok(())
"##,
        r##"100 Continue
101 Switching Protocols
102 Processing
103 Early Hints
200 OK
201 Created
202 Accepted
203 Non-Authoritative Information
204 No Content
205 Reset Content
206 Partial Content
207 Multi-Status
208 Already Reported
226 IM Used
300 Multiple Choices
301 Moved Permanently
302 Found
303 See Other
304 Not Modified
305 Use Proxy
307 Temporary Redirect
308 Permanent Redirect
400 Bad Request
401 Unauthorized
402 Payment Required
403 Forbidden
404 Not Found
405 Method Not Allowed
406 Not Acceptable
407 Proxy Authentication Required
408 Request Timeout
409 Conflict
410 Gone
411 Length Required
412 Precondition Failed
413 Content Too Large
414 URI Too Long
415 Unsupported Media Type
416 Range Not Satisfiable
417 Expectation Failed
421 Misdirected Request
422 Unprocessable Content
423 Locked
424 Failed Dependency
425 Too Early
426 Upgrade Required
428 Precondition Required
429 Too Many Requests
431 Request Header Fields Too Large
451 Unavailable For Legal Reasons
500 Internal Server Error
501 Not Implemented
502 Bad Gateway
503 Service Unavailable
504 Gateway Timeout
505 HTTP Version Not Supported
506 Variant Also Negotiates
507 Insufficient Storage
508 Loop Detected
510 Not Extended
511 Network Authentication Required"##,
    );
}

/// A streamed HEAD response keeps its declared Content-Length, adds no other
/// framing, and ends without writing the body (issue #251).
#[test]
fn head_streams_end_without_writing_the_declared_length() {
    runs(
        SCRIPTED,
        r##"
def main() -> Result[(), std.bytes.BytesError | std.http.server.ServerError | AllocError | IoError]:
    config = std.http.server.config()
    scripted("HEAD /declared HTTP/1.1\r\nHost: x\r\n\r\nGET / HTTP/1.1\r\nHost: x\r\n\r\n", 5, False, &config)?
    Ok(())
"##,
        r##"HTTP/1.1 200 OK|Content-Length: 5||HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 4||home
connections=1 requests=2 handler_errors=0 protocol_errors=0 timeouts=0 cancelled=0 rejected=0"##,
    );
}

/// HEAD responses, whole or streamed, log 0 body bytes (issue #252).
#[test]
fn head_responses_count_no_body_bytes() {
    runs(
        SCRIPTED,
        r##"import std.log
from std.log import Event

def main() -> Result[(), std.bytes.BytesError | AllocError | std.http.server.ServerError | IoError]:
    (sender, receiver) = channel[Event](16)?
    mut config = std.http.server.config()
    config.events = Some(std.log.channel_sink(sender))
    scripted("HEAD / HTTP/1.1\r\nHost: x\r\n\r\nHEAD /sized HTTP/1.1\r\nHost: x\r\n\r\nGET /sized HTTP/1.1\r\nHost: x\r\n\r\n", 64, False, &config)?
    config.events = Nothing
    while True:
        match receiver.recv_nowait():
            case Ok(event):
                mut line = event.message.concat("")?
                for index in range(len(event.fields)):
                    field = &event.fields[index]
                    if field.key == "target" or field.key == "bytes":
                        line = line.concat(" ")?.concat(&field.key)?.concat("=")?.concat(&str.repr(&field.value)?)?
                print(line)?
            case Err(_):
                break
    Ok(())
"##,
        r##"HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 4||HTTP/1.1 200 OK|Content-Length: 5||HTTP/1.1 200 OK|Content-Length: 5||hello
connections=1 requests=3 handler_errors=0 protocol_errors=0 timeouts=0 cancelled=0 rejected=0
request target=std.log.Value.Text("/") bytes=std.log.Value.Integer(0)
request target=std.log.Value.Text("/sized") bytes=std.log.Value.Integer(0)
request target=std.log.Value.Text("/sized") bytes=std.log.Value.Integer(5)"##,
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
def main() -> Result[(), std.bytes.BytesError | std.http.server.ServerError | AllocError | IoError]:
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

connections=1 requests=3 handler_errors=0 protocol_errors=1 timeouts=0 cancelled=0 rejected=0
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
def main() -> Result[(), std.bytes.BytesError | std.http.server.ServerError | AllocError | IoError]:
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

def main() -> Result[(), std.net.NetError | std.http.server.ServerError | SpawnError[()] | AllocError | IoError]:
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

def run(path: str, drain_ms: u32) -> Result[(), std.net.NetError | std.http.server.ServerError | SpawnError[()] | AllocError | IoError]:
    table = routes()?
    app = gated()?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    mut config = std.http.server.config()
    config.drain_timeout = std.time.milliseconds(drain_ms)
    start = std.time.monotonic_now()
    mut summary = ""
    with spawn(client, port, path, stop.share()).drop_job()? as task:
        report = std.http.server.serve(listener, &table, &app, &config, &stop)?
        summary = counts(&report)?
    print(summary)?
    elapsed = std.time.monotonic_now().saturating_duration_since(&start).as_nanoseconds()
    print(elapsed < 3000000000u64)?
    Ok(())

def main() -> Result[(), std.net.NetError | std.http.server.ServerError | SpawnError[()] | AllocError | IoError]:
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

def main() -> Result[(), AllocError | std.net.NetError | std.http.server.ServerError | SpawnError[()] | IoError]:
    table = routes()?
    app = gated()?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    (sender, receiver) = channel[Event](16)?
    mut config = std.http.server.config()
    config.idle_timeout = std.time.milliseconds(200u32)
    config.head_timeout = std.time.milliseconds(200u32)
    config.events = Some(std.log.channel_sink(sender))
    with spawn(client, port, stop.share()).drop_job()? as task:
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
from std.http import HttpError
from std.http.server import Match, Router, ServerError
from std.sync import CancellationToken

def shown(table: &Router[str], method: &str, path: &str) -> Result[(), AllocError | IoError]:
    result = table.find(method, path)
    match &result:
        case Ok(Match.Found(index, _)):
            route = *table.route(*index)
            print(method, path, str.repr(&result)?, route)?
        case _:
            print(method, path, str.repr(&result)?)?
    Ok(())

pub class Nobody:
    pub def handle(self, route: &str, exchange: &mut std.http.server.Exchange[std.net.TcpStream]) -> Result[(), HttpError]:
        Ok(())

def main() -> Result[(), std.net.NetError | std.http.server.ServerError | AllocError | IoError]:
    mut table = std.http.server.router[str]()?
    print(str.repr(&table.add("GET", "/users/{id}", "user"))?)?
    print(str.repr(&table.add("GET", "/users/{other}", "other"))?)?
    print(str.repr(&table.add("PUT", "/users/{id}", "put"))?)?
    print(str.repr(&table.add("GET", "/users/me", "me"))?)?
    print(str.repr(&table.add("GET", "/static/{path...}", "static"))?)?
    print(str.repr(&table.add("GET", "/a/{rest...}/b", "bad"))?)?
    print(str.repr(&table.add("GET", "users", "bad"))?)?
    print(str.repr(&table.add("G T", "/x", "bad"))?)?
    print(str.repr(&table.add("GET", "/{}", "bad"))?)?
    print(str.repr(&table.add("GET", "/x{y}", "bad"))?)?
    print(str.repr(&table.add("GET", "/", "home"))?)?
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
GET /users/42 Result[std.http.server.Match, std.http.server.ServerError].Ok(std.http.server.Match.Found(0, ["42"])) user
GET /users/me Result[std.http.server.Match, std.http.server.ServerError].Ok(std.http.server.Match.Found(0, ["me"])) user
HEAD /users/42 Result[std.http.server.Match, std.http.server.ServerError].Ok(std.http.server.Match.Found(0, ["42"])) user
DELETE /users/42 Result[std.http.server.Match, std.http.server.ServerError].Ok(std.http.server.Match.MethodNotAllowed("GET, PUT, HEAD"))
GET /users/ Result[std.http.server.Match, std.http.server.ServerError].Ok(std.http.server.Match.NotFound)
GET /users/42/ Result[std.http.server.Match, std.http.server.ServerError].Ok(std.http.server.Match.NotFound)
GET /static Result[std.http.server.Match, std.http.server.ServerError].Ok(std.http.server.Match.Found(3, [""])) static
GET /static/ Result[std.http.server.Match, std.http.server.ServerError].Ok(std.http.server.Match.Found(3, [""])) static
GET /static/a/b.css Result[std.http.server.Match, std.http.server.ServerError].Ok(std.http.server.Match.Found(3, ["a/b.css"])) static
GET / Result[std.http.server.Match, std.http.server.ServerError].Ok(std.http.server.Match.Found(4, [])) home
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
def main() -> Result[(), std.net.NetError | std.http.server.ServerError | AllocError | IoError]:
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
        "Result[std.http.server.Report, std.http.server.ServerError].Err(std.http.server.ServerError.Thread(SpawnError[()].Unavailable((), 11)))",
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
def sample() -> Result[Bytes, AllocError | std.bytes.BytesError]:
    Ok(std.bytes.from_text("POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\nX-Long-Field-Name-Beyond-Inline: a-value-beyond-inline-storage\r\n\r\n3\r\nabc\r\n0\r\n\r\nGET /hello/a-name-beyond-inline HTTP/1.1\r\nHost: x\r\n\r\n")?)

def main() -> Result[(), std.bytes.BytesError | std.http.server.ServerError | AllocError | IoError]:
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
    stream.write_all_before(&request, &deadline, &never).unwrap()

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
                # At most 3 rounds of 10 scripted connections.
                unchecked:
                    answered += 1
    print(answered).unwrap()
    stop.cancel()
    mut closed = 0
    for index in range(10):
        if until(&mut streams[index], "").unwrap() == "":
            # The list holds exactly 10 scripted connections.
            unchecked:
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

def main() -> Result[(), std.net.NetError | std.http.server.ServerError | SpawnError[()] | AllocError | IoError]:
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
    with spawn(many, port, stop.share()).drop_job()? as task:
        first = Some(std.http.server.serve(listener, &table, &app, &config, &stop)?)
    print(counts(&first.unwrap())?)?
    config.idle_connections = 2
    config.idle_timeout = std.time.milliseconds(300u32)
    second = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    other = second.local_address()?.port
    halt = CancellationToken()?
    with spawn(capped, other, halt.share()).drop_job()? as task:
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
    stream.write_all_before(&request, &deadline, &never).unwrap()

def millis_since(start: &std.time.Instant) -> u64:
    divisor = NonZero.try_from(1000000u64).unwrap()
    std.time.monotonic_now().saturating_duration_since(start).as_nanoseconds() // divisor

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

def main() -> Result[(), std.net.NetError | std.http.server.ServerError | SpawnError[()] | AllocError | IoError]:
    table = routes()?
    app = gated()?
    mut config = std.http.server.config()
    config.workers = 1
    config.head_timeout = std.time.milliseconds(1000u32)
    config.limits.max_head = 256
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    with spawn(pieces, port, stop.share()).drop_job()? as task:
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
    pub def handle(self, route: &Route, exchange: &mut Exchange[TlsTransport]) -> Result[(), std.http.HttpError]:
        exchange.send_text(200, "secure")?
        Ok(())

def ask(stream: &mut TlsStream) -> str:
    never = CancellationToken().unwrap()
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(5u32)).unwrap()
    request = std.bytes.from_text("GET / HTTP/1.1\r\nHost: x\r\n\r\n").unwrap()
    stream.write_all_before(&request, &deadline, &never).unwrap()
    # Responses are length-delimited; the server sends no close_notify.
    mut out = std.bytes.empty().unwrap()
    while not out.to_text().unwrap().endswith("secure"):
        at = out.len()
        extent = checked:
            at + 64
        end = extent.unwrap()
        out.resize(end, 0u8).unwrap()
        count = stream.read_before(&mut out, at, end, &deadline, &never).unwrap()
        consumed = checked:
            at + count
        out.truncate(consumed.unwrap())
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

def main() -> Result[(), AllocError | std.net.NetError | std.http.server.ServerError | SpawnError[()] | IoError]:
    table = routes()?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    tls = std.tls.server_config(&read_text("FIXTURES/leaf.pem")?, &read_text("FIXTURES/leaf.key")?).unwrap()
    (sender, receiver) = channel[Event](16)?
    mut config = std.http.server.config()
    config.workers = 1
    config.handshake_timeout = std.time.milliseconds(300u32)
    config.events = Some(std.log.channel_sink(sender))
    with spawn(client, port, stop.share()).drop_job()? as task:
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
    pub def handle(self, route: &Route, exchange: &mut Exchange[TlsTransport]) -> Result[(), std.http.HttpError]:
        exchange.send_text(200, "secure")?
        Ok(())

# Sends `raw` and reads until the answer ends with `marker`; the server
# sends no close_notify, so the end of a closed connection is an error.
def ask(stream: &mut TlsStream, raw: &str, marker: &str) -> str:
    never = CancellationToken().unwrap()
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(5u32)).unwrap()
    request = std.bytes.from_text(raw).unwrap()
    stream.write_all_before(&request, &deadline, &never).unwrap()
    mut out = std.bytes.empty().unwrap()
    while marker.byte_len() > 0 and not out.to_text().unwrap().endswith(marker):
        at = out.len()
        extent = checked:
            at + 64
        end = extent.unwrap()
        out.resize(end, 0u8).unwrap()
        count = stream.read_before(&mut out, at, end, &deadline, &never).unwrap()
        consumed = checked:
            at + count
        out.truncate(consumed.unwrap())
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

def client(port: u16, stop: CancellationToken) -> ():
    start = std.time.monotonic_now()
    # A TLS record header promising 512 bytes of handshake, then one byte.
    mut hello = std.net.connect(&std.net.loopback_v4(port), &std.time.deadline_after(&start, &std.time.seconds(5u32)).unwrap(), &CancellationToken().unwrap()).unwrap()
    partial = std.bytes.from_list(&[22u8, 3u8, 1u8, 2u8, 0u8, 1u8].unwrap()).unwrap()
    hello.write_all_before(&partial, &std.time.deadline_after(&start, &std.time.seconds(5u32)).unwrap(), &CancellationToken().unwrap()).unwrap()
    mut stalled = secure(port)
    drop(ask(&mut stalled, "GET / HTTP/1.1\r\nHo", ""))
    mut first = secure(port)
    mut second = secure(port)
    for round in range(2):
        print(ask(&mut first, "GET / HTTP/1.1\r\nHost: x\r\n\r\n", "secure")).unwrap()
        print(ask(&mut second, "GET / HTTP/1.1\r\nHost: x\r\n\r\n", "secure")).unwrap()
    divisor = NonZero.try_from(1000000u64).unwrap()
    waited = std.time.monotonic_now().saturating_duration_since(&start).as_nanoseconds() // divisor
    print(waited < 700u64).unwrap()
    print(ask(&mut stalled, "", "Request Timeout\n")).unwrap()
    print(closes(&mut hello)).unwrap()
    stop.cancel()

def main() -> Result[(), AllocError | std.net.NetError | std.http.server.ServerError | SpawnError[()] | IoError]:
    table = routes()?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    tls = std.tls.server_config(&read_text("FIXTURES/leaf.pem")?, &read_text("FIXTURES/leaf.key")?).unwrap()
    (sender, receiver) = channel[Event](16)?
    mut config = std.http.server.config()
    config.workers = 1
    config.handshake_timeout = std.time.milliseconds(1000u32)
    # Later than the handshake deadline, so the events come in a fixed order.
    config.head_timeout = std.time.milliseconds(1500u32)
    config.events = Some(std.log.channel_sink(sender))
    with spawn(client, port, stop.share()).drop_job()? as task:
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

def attempt(table: &mut Router[i64], method: &str, pattern: &str, path: &str) -> Result[Match, ServerError]:
    table.add(method, pattern, 1)?
    table.find(method, path)

def needed(method: &str, pattern: &str, path: &str) -> Result[i64, std.http.server.ServerError | AllocError | IoError]:
    for budget in range(200):
        mut table = std.http.server.router[i64]()?
        marker = "__test_fail_allocations_after_".concat(str.from(budget)?)?.concat("__")?
        print(marker)?
        outcome = attempt(&mut table, method, pattern, path)
        print("__test_restore_allocations__")?
        if outcome.is_ok():
            return Ok(budget)
    Ok(-1)

def main() -> Result[(), std.http.server.ServerError | AllocError | IoError | OverflowError[i64]]:
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

/// Issue #159: a route value can carry the route's own handler and state,
/// and several patterns can share one route variant.
#[test]
fn route_values_carry_per_route_handlers() {
    runs(
        SCRIPTED,
        r##"
pub enum Site:
    Home
    Greet(Greeter)

pub class Greeter:
    pub opening: str

    pub def handle(self, exchange: &mut Exchange[Script]) -> Result[(), std.http.HttpError | AllocError]:
        text = self.opening.concat(&exchange.params[0])?
        exchange.send_text(200, &text)?
        Ok(())

pub class SiteApp:
    pub def handle(self, route: &Site, exchange: &mut Exchange[Script]) -> Result[(), std.http.HttpError | AllocError]:
        match route:
            case Site.Home:
                exchange.send_text(200, "home")?
            case Site.Greet(greeter):
                greeter.handle(exchange)?
        Ok(())

def main() -> Result[(), std.bytes.BytesError | std.http.server.ServerError | AllocError | IoError]:
    mut table = std.http.server.router[Site]()?
    table.add("GET", "/", Site.Home)?
    table.add("GET", "/index.html", Site.Home)?
    table.add("GET", "/hi/{name}", Site.Greet(Greeter("hi ")))?
    table.add("GET", "/bye/{name}", Site.Greet(Greeter("bye ")))?
    print(str.repr(&table.add("GET", "/hi/{other}", Site.Home))?)?
    input = "GET / HTTP/1.1\r\nHost: x\r\n\r\nGET /index.html HTTP/1.1\r\nHost: x\r\n\r\nGET /hi/ada HTTP/1.1\r\nHost: x\r\n\r\nGET /bye/bob HTTP/1.1\r\nHost: x\r\n\r\n"
    script = Script(std.bytes.from_text(input)?, 0, 7, False, True)
    stop = CancellationToken()?
    report = std.http.server.serve_connection(script, &table, &SiteApp(), &std.http.server.config(), &stop, &stop)
    print("")?
    print(counts(&report)?)?
    Ok(())
"##,
        r##"Result[(), std.http.server.ServerError].Err(std.http.server.ServerError.DuplicateRoute)
HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 4||homeHTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 4||homeHTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 6||hi adaHTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 7||bye bob
connections=1 requests=4 handler_errors=0 protocol_errors=0 timeouts=0 cancelled=0 rejected=0"##,
    );
}

/// Issue #159: dispatch is a `match` on the route type, so a route variant
/// the handler does not answer is a compile error, and a handler must take
/// the router's route type.
#[test]
fn route_dispatch_is_checked_at_compile_time() {
    let program = |arms: &str, route: &str| {
        format!(
            r##"import std.http.server
import std.net
from std.http import HttpError
from std.http.server import Exchange
from std.net import TcpStream
from std.sync import CancellationToken

pub enum Route:
    Home
    Hello(str)

pub enum Other:
    Home

pub class App:
    pub def handle(self, route: &{route}, exchange: &mut Exchange[TcpStream]) -> Result[(), HttpError]:
        match route:
{arms}        Ok(())

def main() -> Result[(), std.net.NetError | std.http.server.ServerError | AllocError]:
    mut routes = std.http.server.router[Route]()?
    routes.add("GET", "/", Route.Home)?
    routes.add("GET", "/hello/{{name}}", Route.Hello("hello"))?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 4)?
    stop = CancellationToken()?
    stop.cancel()
    std.http.server.serve(listener, &routes, &App(), &std.http.server.config(), &stop)?
    Ok(())
"##
        )
    };
    let check = |source: String| {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.plenty");
        std::fs::write(&path, source).unwrap();
        plenty::check_file(&path, None).map_err(|error| error.to_string())
    };
    let home = "            case Route.Home:\n                exchange.send_text(200, \"home\")?\n";
    let hello =
        "            case Route.Hello(text):\n                exchange.send_text(200, text)?\n";
    check(program(&format!("{home}{hello}"), "Route")).unwrap();
    let missing = check(program(home, "Route")).unwrap_err();
    assert!(
        missing.contains("non-exhaustive match; missing Route.Hello(_)"),
        "{missing}"
    );
    let other =
        "            case Other.Home:\n                exchange.send_text(200, \"home\")?\n";
    let mismatched = check(program(other, "Other")).unwrap_err();
    assert!(
        mismatched
            .contains("conflicting types for `R` in `std.http.server.serve`: Route and Other"),
        "{mismatched}"
    );
}

/// Issue #325: `Handler[T, R, E]` takes `E` from `handle`, so a handler
/// keeps its own error type: one `HttpError`, or a sum with an application
/// error. Either `Err` is answered and counted as a `Deliberate` one is, a body
/// the client failed to deliver stays the client's error, routing is
/// unchanged, and a generic caller can write the bound with `E` inferred.
#[test]
fn handlers_return_their_own_error_types() {
    runs(
        SCRIPTED,
        r##"
from std.http.server import Handler

pub enum Refused:
    Teapot

pub class Precise:
    pub def handle(self, route: &Route, exchange: &mut Exchange[Script]) -> Result[(), HttpError]:
        match route:
            case Route.Home:
                exchange.send_text(200, "precise")?
            case _:
                return Err(HttpError.Broken)
        Ok(())

pub class Summed:
    pub def handle(self, route: &Route, exchange: &mut Exchange[Script]) -> Result[(), Refused | HttpError | AllocError | std.bytes.BytesError]:
        match route:
            case Route.Param:
                text = "summed ".concat(&exchange.params[0])?
                exchange.send_text(200, &text)?
            case Route.Echo:
                mut body = std.bytes.empty()?
                exchange.read_all(&mut body)?
                exchange.send(200, "application/octet-stream", &body)?
            case Route.Broken:
                exchange.start(std.http.response(200, "OK")?)?
                piece = std.bytes.from_text("part")?
                exchange.write(&piece[:4])?
                return Err(Refused.Teapot)
            case _:
                return Err(Refused.Teapot)
        Ok(())

# Generic over any handler of the scripted routes, whatever its error type.
def play[E, H: Handler[Script, Route, E]](handler: &H, input: &str, config: &Config) -> Result[(), std.bytes.BytesError | std.http.server.ServerError | AllocError | IoError]:
    table = routes()?
    stop = CancellationToken()?
    cancel = CancellationToken()?
    script = Script(std.bytes.from_text(input)?, 0, 64, False, True)
    report = std.http.server.serve_connection(script, &table, handler, config, &stop, &cancel)
    print("")?
    print(counts(&report)?)?
    Ok(())

def main() -> Result[(), std.bytes.BytesError | std.http.server.ServerError | AllocError | IoError]:
    mut config = std.http.server.config()
    config.limits.max_body = 4
    print("--- one error")?
    play(&Precise(), "GET / HTTP/1.1\r\nHost: x\r\n\r\nGET /fail HTTP/1.1\r\nHost: x\r\n\r\n", &config)?
    print("--- a sum")?
    play(&Summed(), "GET /hello/Ada HTTP/1.1\r\nHost: x\r\n\r\nDELETE /echo HTTP/1.1\r\nHost: x\r\n\r\nGET /missing HTTP/1.1\r\nHost: x\r\n\r\nGET /fail HTTP/1.1\r\nHost: x\r\n\r\n", &config)?
    print("--- body limit")?
    play(&Summed(), "POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n", &config)?
    print("--- broken")?
    play(&Summed(), "GET /broken HTTP/1.1\r\nHost: x\r\n\r\n", &config)?
    Ok(())
"##,
        r##"--- one error
HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 7||preciseHTTP/1.1 500 Internal Server Error|Content-Type: text/plain; charset=utf-8|Content-Length: 22|Connection: close||Internal Server Error

connections=1 requests=2 handler_errors=1 protocol_errors=0 timeouts=0 cancelled=0 rejected=0
--- a sum
HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 10||summed AdaHTTP/1.1 405 Method Not Allowed|Allow: POST|Content-Type: text/plain; charset=utf-8|Content-Length: 19||Method Not Allowed
HTTP/1.1 404 Not Found|Content-Type: text/plain; charset=utf-8|Content-Length: 10||Not Found
HTTP/1.1 500 Internal Server Error|Content-Type: text/plain; charset=utf-8|Content-Length: 22|Connection: close||Internal Server Error

connections=1 requests=4 handler_errors=1 protocol_errors=0 timeouts=0 cancelled=0 rejected=0
--- body limit
HTTP/1.1 413 Content Too Large|Content-Type: text/plain; charset=utf-8|Content-Length: 18|Connection: close||Content Too Large

connections=1 requests=1 handler_errors=0 protocol_errors=1 timeouts=0 cancelled=0 rejected=0
--- broken
HTTP/1.1 200 OK|Transfer-Encoding: chunked||4|part|
connections=1 requests=1 handler_errors=1 protocol_errors=0 timeouts=0 cancelled=0 rejected=0"##,
    );
}

/// Issue #325: typed handler errors over plain TCP `serve` and HTTPS
/// `std.http.tls.serve`, where each worker thread drops the error it gets.
#[test]
fn served_handlers_return_their_own_error_types() {
    let main = r##"import std.http.tls
import std.tls
from std.http import HttpError
from std.http.tls import TlsTransport
from std.tls import TlsStream

pub enum Refused:
    Teapot

pub class Plain:
    pub def handle(self, route: &Route, exchange: &mut Exchange[TcpStream]) -> Result[(), Refused | HttpError]:
        match route:
            case Route.Home:
                exchange.send_text(200, "plain")?
            case _:
                return Err(Refused.Teapot)
        Ok(())

pub class Secure:
    pub def handle(self, route: &Route, exchange: &mut Exchange[TlsTransport]) -> Result[(), HttpError]:
        match route:
            case Route.Home:
                exchange.send_text(200, "secure")?
            case _:
                return Err(HttpError.Broken)
        Ok(())

def plain_client(port: u16, stop: CancellationToken) -> ():
    print(fetch(port, "GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap()).unwrap()
    print(fetch(port, "GET /wait HTTP/1.1\r\nHost: x\r\n\r\n").unwrap()).unwrap()
    stop.cancel()

# Responses are length-delimited and the server sends no close_notify, so
# read until the body's last text.
def ask(port: u16, path: &str, last: &str) -> str:
    never = CancellationToken().unwrap()
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(5u32)).unwrap()
    roots = std.tls.roots_from_pem(&read_text("FIXTURES/ca.pem").unwrap()).unwrap()
    config = std.tls.client_config(&roots).unwrap()
    tcp = std.net.connect(&std.net.loopback_v4(port), &deadline, &never).unwrap()
    mut stream = std.tls.connect(tcp, &config, "localhost", &deadline, &never).unwrap()
    request = std.bytes.from_text(&"GET ".concat(path).unwrap().concat(" HTTP/1.1\r\nHost: x\r\n\r\n").unwrap()).unwrap()
    stream.write_all_before(&request, &deadline, &never).unwrap()
    mut out = std.bytes.empty().unwrap()
    while not out.to_text().unwrap().endswith(last):
        at = out.len()
        extent = checked:
            at + 64
        end = extent.unwrap()
        out.resize(end, 0u8).unwrap()
        count = stream.read_before(&mut out, at, end, &deadline, &never).unwrap()
        consumed = checked:
            at + count
        out.truncate(consumed.unwrap())
    out.to_text().unwrap().replace("\r\n", "|").unwrap()

def secure_client(port: u16, stop: CancellationToken) -> ():
    print(ask(port, "/", "secure")).unwrap()
    print(ask(port, "/wait", "Error\n")).unwrap()
    stop.cancel()

def main() -> Result[(), std.net.NetError | std.http.server.ServerError | SpawnError[()] | AllocError | IoError]:
    table = routes()?
    mut config = std.http.server.config()
    config.workers = 1
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    with spawn(plain_client, port, stop.share()).drop_job()? as task:
        print(counts(&std.http.server.serve(listener, &table, &Plain(), &config, &stop)?)?)?
    secure = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    secure_port = secure.local_address()?.port
    secure_stop = CancellationToken()?
    tls = std.tls.server_config(&read_text("FIXTURES/leaf.pem")?, &read_text("FIXTURES/leaf.key")?).unwrap()
    with spawn(secure_client, secure_port, secure_stop.share()).drop_job()? as task:
        print(counts(&std.http.tls.serve(secure, tls, &table, &Secure(), &config, &secure_stop)?)?)?
    Ok(())
"##
    .replace("FIXTURES", &fixtures());
    runs(
        LOOPBACK,
        &main,
        r##"HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 5|Connection: close||plain
HTTP/1.1 500 Internal Server Error|Content-Type: text/plain; charset=utf-8|Content-Length: 22|Connection: close||Internal Server Error

connections=2 requests=2 handler_errors=1 protocol_errors=0 timeouts=0 cancelled=0 rejected=0
HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 6||secure
HTTP/1.1 500 Internal Server Error|Content-Type: text/plain; charset=utf-8|Content-Length: 22|Connection: close||Internal Server Error

connections=2 requests=2 handler_errors=1 protocol_errors=0 timeouts=0 cancelled=0 rejected=0"##,
    );
}

/// Issue #325: `handle` may return any `Result[(), E]`; a method returning
/// anything else does not satisfy `Handler`, and the error names it.
#[test]
fn a_handler_must_return_a_unit_result() {
    let program = |output: &str, body: &str| {
        format!(
            r##"import std.http
import std.http.server
import std.net
from std.http.server import Exchange
from std.net import TcpStream
from std.sync import CancellationToken

pub enum Route:
    Home

pub class App:
    pub def handle(self, route: &Route, exchange: &mut Exchange[TcpStream]) -> {output}:
{body}

def main() -> Result[(), std.net.NetError | std.http.server.ServerError | AllocError]:
    mut routes = std.http.server.router[Route]()?
    routes.add("GET", "/", Route.Home)?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 4)?
    stop = CancellationToken()?
    stop.cancel()
    std.http.server.serve(listener, &routes, &App(), &std.http.server.config(), &stop)?
    Ok(())
"##
        )
    };
    let check = |source: String| {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.plenty");
        std::fs::write(&path, source).unwrap();
        plenty::check_file(&path, None).map_err(|error| error.to_string())
    };
    let sends = "        exchange.send_text(200, \"home\")?\n";
    check(program(
        "Result[(), std.http.HttpError]",
        &format!("{sends}        Ok(())"),
    ))
    .unwrap();
    for (output, body) in [
        (
            "()",
            "        drop(exchange.send_text(200, \"home\"))".to_owned(),
        ),
        (
            "Result[i64, std.http.HttpError]",
            format!("{sends}        Ok(1)"),
        ),
    ] {
        let error = check(program(output, &body)).unwrap_err();
        assert!(
            error.contains(
                "App does not satisfy std.http.server.Handler: signature of `handle` does not match"
            ),
            "{error}"
        );
    }
}

/// Test private representation boundaries without constructing enormous bodies
/// or waiting for a server to process 2^64 requests.
#[test]
fn checked_server_extents_and_observation_counts_in_both_build_modes() {
    let library = include_str!("../stdlib/std/http/server.plenty");
    let helper = |name: &str| {
        library[library.find(name).unwrap()..]
            .split("\n\n")
            .next()
            .unwrap()
    };
    let report = helper("pub class Report:");
    let counted = helper("def counted(");
    let merge = helper("def merge(");
    let unread = helper("def unread_extent(");
    let sent = helper("def sent_extent(");
    let after = helper("def after(");
    let gathered = helper("class Gathered:");
    let scanned = helper("def scanned(");
    let send_all = helper("def send_all[");
    let least = helper("def least(");
    let deadline_of = helper("def deadline_of(");
    let now_ns = helper("def now_ns(");
    let write_start = library
        .find("    pub def write(self: &mut Exchange[T]")
        .unwrap();
    let write = library[write_start..].split("\n\n").next().unwrap();
    let write = write.replace("Exchange[T]", "Probe");
    let source = format!(
        r#"import std.http
import std.bytes
from std.bytes import Bytes
from std.http import HttpError, Limits
from std.net import NetError
from std.io import StreamError
from std.sync import CancellationToken
from std.time import Deadline, deadline_at, instant_at, monotonic_now
{report}
{counted}
{merge}
{unread}
{sent}
{after}
{least}
{deadline_of}
{now_ns}
protocol Transport:
    def write_before(self: &mut Transport, source: &[u8], deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, NetError]:
        pass
class Writer:
    count: i64

    def write_before(self: &mut Writer, source: &[u8], deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, NetError]:
        Ok(self.count)

{send_all}
{gathered}
const line_feed: u8 = 10u8
const carriage_return: u8 = 13u8
{scanned}
const sized: i64 = 1
const chunked: i64 = 2
const closing: i64 = 3
const broken: i64 = 4
class Probe:
    _state: i64
    _remaining: i64
    _sent: i64
    _head_only: bool
    called: bool

    def put(self: &mut Probe, source: &[u8], framed: bool) -> Result[(), HttpError]:
        self.called = True
        Ok(())

    def flush(self: &mut Probe) -> Result[(), HttpError]:
        Ok(())

{write}

def main() -> Result[(), IoError | AllocError]:
    never = CancellationToken()?
    mut oversized = Writer(i64.MAX)
    mut short = Writer(1)
    mut zero = Writer(0)
    limits = Limits(i64.MAX, i64.MAX, NonNegative.try_from(i64.MAX).unwrap(), 0)
    mut input = Gathered(std.bytes.from_text("X: y\r\n\r\n")?, 0, 0, i64.MAX, 9223372036854775806)
    mut total = Report(18446744073709551614u64, 0u64, u64.MAX, 1u64, 5u64, 6u64, 7u64, 8u64, 9u64)
    part = Report(1u64, u64.MAX, 1u64, u64.MAX, 2u64, 3u64, 4u64, 5u64, 6u64)
    mut stream = Probe(chunked, 0, i64.MAX, False, False)
    mut head = Probe(closing, 0, i64.MAX, True, False)
    mut exact_stream = Probe(sized, 1, 9223372036854775806, False, False)
    print("__test_begin_no_allocations__")?
    transport_range = match send_all(&mut oversized, b"xx", 0u64, 0u64, &never):
        case Err(NetError.Stream(StreamError.OutOfRange)):
            True
        case _:
            False
    transport_zero = match send_all(&mut zero, b"xx", 0u64, 0u64, &never):
        case Err(NetError.Stream(StreamError.WriteZero)):
            True
        case _:
            False
    transport_short = send_all(&mut short, b"xx", 0u64, 0u64, &never).is_ok()
    scan = scanned(&mut input, &limits) and input.fields == i64.MAX and input.line == 8
    increment = counted(u64.MAX, 1u64) == u64.MAX and counted(18446744073709551614u64, 1u64) == u64.MAX
    merge(&mut total, &part)
    merging = total.connections == u64.MAX and total.rejected == u64.MAX and total.requests == u64.MAX and total.handler_errors == u64.MAX and total.protocol_errors == 7u64 and total.timeouts == 9u64 and total.cancelled == 11u64 and total.accept_errors == 13u64 and total.dropped_events == 15u64
    merge(&mut total, &part)
    repeated = total.connections == u64.MAX and total.rejected == u64.MAX and total.requests == u64.MAX and total.handler_errors == u64.MAX
    allocation = unread_extent(i64.MAX, 10, 10).unwrap() == i64.MAX and unread_extent(i64.MAX, 1, 0).is_err()
    exact = sent_extent(9223372036854775806, 1).unwrap() == i64.MAX and sent_extent(i64.MAX, 0).unwrap() == i64.MAX
    rejected = match stream.write(b"x"):
        case Err(HttpError.BodyTooLarge):
            True
        case _:
            False
    preserved = not stream.called and stream._state == chunked and stream._sent == i64.MAX and stream._remaining == 0
    head_ok = head.write(b"x").is_ok() and head._sent == i64.MAX and head._remaining == 0
    exact_write = exact_stream.write(b"x").is_ok() and exact_stream._sent == i64.MAX and exact_stream._remaining == 0 and exact_stream._state == sized and exact_stream.called
    deadlines = after(u64.MAX, 0u64) == u64.MAX and after(u64.MAX, 1u64) == u64.MAX and after(18446744073709551614u64, 1u64) == u64.MAX and after(18446744073709551614u64, 2u64) == u64.MAX and after(5u64, 7u64) == 12u64
    print("__test_end_no_allocations__")?
    print(transport_range, transport_zero, transport_short, scan, increment, merging, repeated, allocation, exact, rejected, preserved, head_ok, exact_write, deadlines)?
    Ok(())
"#
    );
    for optimize in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.plenty");
        let binary = dir.path().join("program");
        std::fs::write(&path, &source).unwrap();
        let options = plenty::CompileOptions {
            optimize,
            test_hooks: true,
            ..Default::default()
        };
        plenty::compile_file_to_executable_with_options(&path, &binary, None, &options)
            .unwrap_or_else(|error| panic!("{source}\n{error}"));
        let output = std::process::Command::new(binary).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let lines: Vec<_> = output
            .stdout
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty() && !line.starts_with(b"__test_"))
            .collect();
        assert_eq!(
            lines,
            vec![
                b"True True True True True True True True True True True True True True".as_slice()
            ]
        );
    }
}

// Issue #784: policies keep the application's route type and Handler protocol.
const ROUTING_POLICIES: &str = r##"
pub enum PolicyRoute:
    Normal
    Missing(str)
    WrongMethod
    Fail
    Silent

pub class PolicyApp:
    pub def handle(self, route: &PolicyRoute, exchange: &mut Exchange[Script]) -> Result[(), HttpError | AllocError]:
        match route:
            case PolicyRoute.Normal:
                exchange.send_text(200, &f"normal {exchange.path} {exchange.query} {len(exchange.params)} {exchange.allowed_methods}\n"?)?
            case PolicyRoute.Missing(label):
                exchange.send_text(404, &f"{label} {exchange.path} {exchange.query} {len(exchange.params)}\n"?)?
            case PolicyRoute.WrongMethod:
                status = 204 if exchange.request.method == "OPTIONS" else 405
                mut head = std.http.response(status, std.http.server.reason(status))?
                head.add("Allow", exchange.allowed_methods)?
                head.add("X-Policy", "method")?
                exchange.respond(head, b"")?
            case PolicyRoute.Fail:
                return Err(HttpError.Broken)
            case PolicyRoute.Silent:
                pass
        Ok(())

def policy_request(table: &Router[PolicyRoute], label: &str, method: &str, target: &str) -> Result[(), AllocError | IoError]:
    input = std.bytes.from_text(&f"{method} {target} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"?)?
    stop = CancellationToken()?
    print("BEGIN", label)?
    report = std.http.server.serve_connection(Script(input, 0, 7, False, True), table, &PolicyApp(), &std.http.server.config(), &stop, &stop)
    print("\nEND", label, report.requests, report.handler_errors)?
    Ok(())
"##;

fn policy_section<'a>(output: &'a str, label: &str) -> &'a str {
    output
        .split_once(&format!("BEGIN {label}\n"))
        .unwrap_or_else(|| panic!("missing {label}: {output}"))
        .1
        .split_once(&format!("\nEND {label}"))
        .unwrap()
        .0
}

#[test]
fn typed_fallbacks_customize_missing_paths_methods_and_options() {
    let result = support::http_program_output(&format!(
        "{SCRIPTED}{ROUTING_POLICIES}{}",
        r##"
def main() -> Result[(), ServerError | AllocError | IoError]:
    mut table = std.http.server.router[PolicyRoute]()?
    table.add("GET", "/users/{id}", PolicyRoute.Normal)?
    table.add("PUT", "/users/{id}", PolicyRoute.Normal)?
    policy_request(&table, "default-missing", "GET", "/missing")?
    policy_request(&table, "default-method", "POST", "/users/42")?
    table.set_fallback(Some(PolicyRoute.Missing("custom-missing")))
    table.set_method_not_allowed(Some(PolicyRoute.WrongMethod))
    policy_request(&table, "missing", "POST", "/missing?raw=%2f+%20")?
    policy_request(&table, "method", "POST", "/users/42?raw=%2f")?
    policy_request(&table, "options", "OPTIONS", "/users/42")?
    policy_request(&table, "head", "HEAD", "/missing")?
    policy_request(&table, "normal", "HEAD", "/users/42")?
    policy_request(&table, "invalid-capture", "GET", "/users/%ff")?
    table.set_fallback(Some(PolicyRoute.Missing("replacement")))
    policy_request(&table, "replacement", "GET", "/missing")?
    table.set_fallback(Nothing)
    table.set_method_not_allowed(Nothing)
    policy_request(&table, "cleared-missing", "GET", "/missing")?
    policy_request(&table, "cleared-method", "POST", "/users/42")?
    table.set_method_not_allowed(Some(PolicyRoute.WrongMethod))
    table.add("OPTIONS", "/users/{id}", PolicyRoute.Normal)?
    policy_request(&table, "explicit-options", "OPTIONS", "/users/42")?
    Ok(())
"##
    ));
    for label in ["default-missing", "cleared-missing"] {
        let response = policy_section(&result, label);
        assert!(response.starts_with("HTTP/1.1 404 Not Found|"), "{result}");
        assert!(response.ends_with("Not Found\n"), "{result}");
    }
    for label in ["default-method", "cleared-method"] {
        let response = policy_section(&result, label);
        assert!(
            response.starts_with("HTTP/1.1 405 Method Not Allowed|"),
            "{result}"
        );
        assert!(response.contains("Allow: GET, PUT, HEAD|"), "{result}");
        assert!(!response.contains("X-Policy"), "{result}");
    }
    assert!(
        policy_section(&result, "missing").ends_with("custom-missing /missing raw=%2f+%20 0\n"),
        "{result}"
    );
    let method = policy_section(&result, "method");
    assert!(
        method.starts_with("HTTP/1.1 405 Method Not Allowed|"),
        "{result}"
    );
    assert!(
        method.contains("Allow: GET, PUT, HEAD|X-Policy: method|"),
        "{result}"
    );
    let options = policy_section(&result, "options");
    assert!(options.starts_with("HTTP/1.1 204 No Content|"), "{result}");
    assert!(
        options.contains("Allow: GET, PUT, HEAD|X-Policy: method|"),
        "{result}"
    );
    for label in ["head", "normal"] {
        let response = policy_section(&result, label);
        assert!(response.ends_with("||"), "{result}");
        assert!(!response.contains("custom-missing /missing"), "{result}");
    }
    assert!(
        policy_section(&result, "invalid-capture").starts_with("HTTP/1.1 400 Bad Request|"),
        "{result}"
    );
    assert!(
        policy_section(&result, "replacement").ends_with("replacement /missing  0\n"),
        "{result}"
    );
    assert!(
        policy_section(&result, "explicit-options").starts_with("HTTP/1.1 200 OK|"),
        "{result}"
    );
    assert!(
        result
            .lines()
            .filter(|line| line.starts_with("END "))
            .all(|line| line.ends_with(" 1 0")),
        "{result}"
    );
}

#[test]
fn slash_redirects_preserve_method_query_and_exact_route_precedence() {
    let result = support::http_program_output(&format!(
        "{SCRIPTED}{ROUTING_POLICIES}{}",
        r##"
def main() -> Result[(), ServerError | AllocError | IoError]:
    mut table = std.http.server.router[PolicyRoute]()?
    table.add("GET", "/docs/", PolicyRoute.Normal)?
    table.add("POST", "/submit/", PolicyRoute.Normal)?
    table.add("GET", "/remove", PolicyRoute.Normal)?
    table.add("GET", "/both", PolicyRoute.Normal)?
    table.add("POST", "/both/", PolicyRoute.Normal)?
    table.add("GET", "/users/{id}/", PolicyRoute.Normal)?
    table.add("*", "/wild/", PolicyRoute.Normal)?
    table.add("GET", "/rest/{path...}", PolicyRoute.Normal)?
    table.add("GET", "//evil.example/", PolicyRoute.Normal)?
    table.add("GET", "/back\\slash/", PolicyRoute.Normal)?
    table.add("GET", "/fragment#/", PolicyRoute.Normal)?
    table.set_fallback(Some(PolicyRoute.Missing("missing")))
    policy_request(&table, "exact-default", "GET", "/docs")?
    table.set_trailing_slash(std.http.server.TrailingSlash.Redirect)
    policy_request(&table, "add", "GET", "/docs?raw=%2f+%20&x=a%26b")?
    policy_request(&table, "empty-query", "GET", "/docs?")?
    policy_request(&table, "post", "POST", "/submit?x=1")?
    policy_request(&table, "remove", "GET", "/remove/")?
    policy_request(&table, "head", "HEAD", "/docs")?
    policy_request(&table, "capture", "GET", "/users/Ada%20L")?
    policy_request(&table, "wild", "OPTIONS", "/wild")?
    policy_request(&table, "wrong-alternate-method", "OPTIONS", "/docs")?
    policy_request(&table, "exact-wrong-method", "POST", "/both")?
    policy_request(&table, "exact", "GET", "/docs/")?
    policy_request(&table, "rest", "GET", "/rest")?
    policy_request(&table, "root", "GET", "/")?
    policy_request(&table, "double-slash", "GET", "//evil.example")?
    policy_request(&table, "backslash", "GET", "/back\\slash")?
    policy_request(&table, "fragment", "GET", "/fragment#")?
    policy_request(&table, "absolute", "GET", "http://example.test/docs?x=%2F")?
    table.set_trailing_slash(std.http.server.TrailingSlash.Exact)
    policy_request(&table, "disabled", "GET", "/docs")?
    Ok(())
"##
    ));
    for (label, location) in [
        ("add", "/docs/?raw=%2f+%20&x=a%26b"),
        ("empty-query", "/docs/?"),
        ("post", "/submit/?x=1"),
        ("remove", "/remove"),
        ("head", "/docs/"),
        ("capture", "/users/Ada%20L/"),
        ("wild", "/wild/"),
        ("absolute", "/docs/?x=%2F"),
    ] {
        let response = policy_section(&result, label);
        assert!(
            response.starts_with("HTTP/1.1 308 Permanent Redirect|"),
            "{result}"
        );
        assert!(
            response.contains(&format!("Location: {location}|")),
            "{result}"
        );
        assert!(response.ends_with("||"), "{result}");
        assert!(response.contains("Content-Length: 0|"), "{result}");
    }
    for label in [
        "exact-default",
        "wrong-alternate-method",
        "root",
        "double-slash",
        "backslash",
        "fragment",
        "disabled",
    ] {
        assert!(
            policy_section(&result, label).starts_with("HTTP/1.1 404 Not Found|"),
            "{result}"
        );
    }
    assert!(
        policy_section(&result, "exact-wrong-method")
            .starts_with("HTTP/1.1 405 Method Not Allowed|Allow: GET, HEAD|"),
        "{result}"
    );
    for label in ["exact", "rest"] {
        assert!(
            policy_section(&result, label).starts_with("HTTP/1.1 200 OK|"),
            "{result}"
        );
    }
}

#[test]
fn fallback_failures_follow_the_normal_handler_contract() {
    let result = support::http_program_output(&format!(
        "{SCRIPTED}{ROUTING_POLICIES}{}",
        r##"
def main() -> Result[(), ServerError | AllocError | IoError]:
    mut table = std.http.server.router[PolicyRoute]()?
    table.add("GET", "/", PolicyRoute.Normal)?
    table.set_fallback(Some(PolicyRoute.Fail))
    policy_request(&table, "failed", "GET", "/missing")?
    table.set_fallback(Some(PolicyRoute.Silent))
    policy_request(&table, "silent", "GET", "/missing")?
    table.set_method_not_allowed(Some(PolicyRoute.Fail))
    policy_request(&table, "method-failed", "POST", "/")?
    Ok(())
"##
    ));
    for label in ["failed", "silent", "method-failed"] {
        assert!(
            policy_section(&result, label).starts_with("HTTP/1.1 500 Internal Server Error|"),
            "{result}"
        );
        assert!(result.contains(&format!("END {label} 1 1")), "{result}");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn routing_policy_allocations_fail_without_trapping() {
    let result = output(&format!(
        "{SCRIPTED}{ROUTING_POLICIES}{}",
        r##"
def main() -> Result[(), ServerError | AllocError | IoError]:
    mut table = std.http.server.router[PolicyRoute]()?
    table.add("POST", "/long-canonical-route-beyond-inline/", PolicyRoute.Normal)?
    table.set_trailing_slash(std.http.server.TrailingSlash.Redirect)
    table.set_fallback(Some(PolicyRoute.Missing("missing-label-beyond-inline")))
    table.set_method_not_allowed(Some(PolicyRoute.WrongMethod))
    stop = CancellationToken()?
    config = std.http.server.config()
    for target in ["/long-canonical-route-beyond-inline?query=long-value-beyond-inline%2f", "/missing-path-beyond-inline", "/long-canonical-route-beyond-inline/"]?:
        for budget in range(160):
            method = "GET" if target.endswith("/") else "POST"
            request = std.bytes.from_text(&f"{method} {target} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"?)?
            marker = "__test_fail_allocations_after_".concat(str.from(budget)?)?.concat("__")?
            print(marker)?
            report = std.http.server.serve_connection(Script(request, 0, 7, False, False), &table, &PolicyApp(), &config, &stop, &stop)
            print("__test_restore_allocations__")?
            print(report.requests)?
    Ok(())
"##
    ));
    let counts: Vec<_> = result.lines().collect();
    assert_eq!(counts.len(), 480);
    for group in counts.as_chunks::<160>().0 {
        assert!(group.iter().all(|count| ["0", "1"].contains(count)));
        assert_eq!(group[0], "0");
        assert!(group[140..].iter().all(|count| *count == "1"));
    }
}

#[test]
fn fallback_values_can_own_application_state() {
    let result = support::http_program_output(&format!(
        "{SCRIPTED}{}",
        r##"
pub enum OwnedRoute:
    Missing(list[str])

pub class OwnedApp:
    pub def handle(self, route: &OwnedRoute, exchange: &mut Exchange[Script]) -> Result[(), HttpError]:
        match route:
            case OwnedRoute.Missing(labels):
                exchange.send_text(404, &labels[0])

def main() -> Result[(), ServerError | AllocError | IoError]:
    mut table = std.http.server.router[OwnedRoute]()?
    stop = CancellationToken()?
    for label in ["first-owned-fallback", "second-owned-fallback"]?:
        table.set_fallback(Some(OwnedRoute.Missing([label]?)))
        input = std.bytes.from_text("GET /missing HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")?
        drop(std.http.server.serve_connection(Script(input, 0, 7, False, True), &table, &OwnedApp(), &std.http.server.config(), &stop, &stop))
    table.set_fallback(Nothing)
    Ok(())
"##
    ));
    assert_eq!(result.matches("HTTP/1.1 404 Not Found|").count(), 2);
    assert!(result.contains("||first-owned-fallback"), "{result}");
    assert!(result.ends_with("||second-owned-fallback"), "{result}");
}

/// Date is generated once on every final response path, never on 100,
/// and a mixed-case caller override remains byte-for-byte unchanged.
#[test]
fn response_dates_cover_final_refusal_upgrade_and_detached_paths() {
    let source = SCRIPTED.to_owned();
    let source = source
        + r##"
pub class DateApp:
    pub def handle(self, route: &i64, exchange: &mut Exchange[Script]) -> Result[(), HttpError | AllocError]:
        match *route:
            case 1:
                mut head = std.http.response(200, "OK")?
                head.add("dAtE", "caller value")?
                exchange.respond(head, b"override")?
            case 2:
                mut head = std.http.response(101, "Switching Protocols")?
                head.add("Connection", "Upgrade")?
                head.add("Upgrade", "example")?
                drop(exchange.switch_protocols(head)?)
            case 3:
                drop(exchange.detach(std.http.response(200, "OK")?)?)
            case 4:
                exchange.start(std.http.response(200, "OK")?)?
                exchange.write(b"stream")?
                exchange.end()?
            case 5:
                mut body = std.bytes.empty()?
                exchange.read_all(&mut body)?
                exchange.send_text(200, "continue")?
            case 6:
                return Err(HttpError.Broken)
            case _:
                exchange.send_text(200, "normal")?
        Ok(())

def main() -> Result[(), ServerError | AllocError | IoError]:
    mut table = std.http.server.router[i64]()?
    table.add("GET", "/", 0)?
    table.add("GET", "/bad/{name}", 0)?
    table.add("GET", "/override", 1)?
    table.add("GET", "/upgrade", 2)?
    table.add("GET", "/detach", 3)?
    table.add("GET", "/stream", 4)?
    table.add("POST", "/continue", 5)?
    table.add("GET", "/fail", 6)?
    for line in ["GET /", "GET /override", "GET /upgrade", "GET /detach", "GET /stream", "HEAD /", "GET /fail", "GET /missing", "GET /bad/%zz"]?:
        print("CASE", line)?
        request = line.concat(" HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")?
        stop = CancellationToken()?
        drop(std.http.server.serve_connection(Script(std.bytes.from_text(&request)?, 0, 4096, False, True), &table, &DateApp(), &std.http.server.config(), &stop, &stop))
        print("")?
    print("CASE continue")?
    stop = CancellationToken()?
    drop(std.http.server.serve_connection(Script(std.bytes.from_text("POST /continue HTTP/1.1\r\nHost: x\r\nContent-Length: 1\r\nExpect: 100-continue\r\nConnection: close\r\n\r\nx")?, 0, 1, False, True), &table, &DateApp(), &std.http.server.config(), &stop, &stop))
    print("")?
    Ok(())
"##;
    let result = support::http_program_output(&source);
    let sections: Vec<_> = result.split("CASE ").skip(1).collect();
    assert_eq!(sections.len(), 10, "{result}");
    for section in sections {
        if section.starts_with("GET /override\n") {
            assert_eq!(
                section.matches("dAtE: caller value|").count(),
                1,
                "{section}"
            );
            assert!(!section.contains("Date: "), "{section}");
        } else {
            let dates: Vec<_> = section
                .split('|')
                .filter_map(|field| field.strip_prefix("Date: "))
                .collect();
            assert_eq!(dates.len(), 1, "{section}");
            let date = dates[0];
            assert_eq!(date.len(), 29, "{date}");
            assert!(date.ends_with(" GMT"), "{date}");
            assert_eq!(&date[3..5], ", ");
            assert_eq!(&date[16..17], " ");
        }
    }
    assert!(
        result.contains("HTTP/1.1 100 Continue||HTTP/1.1 200 OK|"),
        "{result}"
    );
    assert!(
        result.contains("HTTP/1.1 101 Switching Protocols|"),
        "{result}"
    );
    assert!(result.contains("HTTP/1.1 400 Bad Request|"), "{result}");
    assert!(
        result.contains("HTTP/1.1 500 Internal Server Error|"),
        "{result}"
    );
}

/// Unusable readings and caller overrides need no allocation. Allocation
/// refusal from a usable reading stays typed instead of producing fake dates.
#[cfg(feature = "runtime-checks")]
#[test]
fn response_date_policy_handles_unusable_time_and_allocation_refusal() {
    let module = include_str!("../stdlib/std/http/server.plenty");
    let start = module.find("def dated(").unwrap();
    let helper = module[start..].split("\n\n").next().unwrap();
    let extra = [
        "def date(",
        "def http_allocated[T](",
        "def flushed(",
        "def admission_response(",
    ]
    .iter()
    .map(|marker| {
        let start = module.find(marker).unwrap();
        module[start..]
            .split("\n\n")
            .next()
            .unwrap()
            .replace("reason(status)", "std.http.server.reason(status)")
    })
    .collect::<Vec<_>>()
    .join("\n\n");
    let source = format!(
        r##"import std.bytes
import std.http
import std.http.server
import std.io
import std.time
from std.bytes import Bytes
from std.io import StreamError
from std.http import ResponseHead, HttpError
{helper}
{extra}

def main() -> Result[(), HttpError | AllocError | IoError]:
    mut bad = std.http.response(200, "OK")?
    mut override = std.http.response(200, "OK")?
    override.add("dAtE", "kept")?
    mut interim = std.http.response(100, "Continue")?
    print("__test_begin_no_allocations__")?
    before = dated(&mut bad, &std.time.Timestamp(-1, 0u32)).is_ok()
    after = dated(&mut bad, &std.time.Timestamp(253402300800, 0u32)).is_ok()
    invalid = dated(&mut bad, &std.time.Timestamp(0, 1000000000u32)).is_ok()
    kept = dated(&mut override, &std.time.Timestamp(0, 0u32)).is_ok()
    skipped = dated(&mut interim, &std.time.Timestamp(0, 0u32)).is_ok()
    print("__test_end_no_allocations__")?
    print("__test_fail_allocations_after_0__")?
    failed = match dated(&mut bad, &std.time.Timestamp(0, 0u32)):
        case Err(HttpError.Allocation(_)):
            True
        case _:
            False
    print("__test_restore_allocations__")?
    print(before, after, invalid, kept, skipped, failed, len(bad.headers), len(override.headers), len(interim.headers))?
    print(std.http.http_date(&std.time.Timestamp(784111777, 0u32))?.unwrap())?
    for status in [408, 503]?:
        text = admission_response(status)?.to_text().unwrap()
        print(len(text.split("\r\nDate: ")?) == 2, "Connection: close\r\n" in text)?
    Ok(())
"##
    );
    assert_eq!(
        support::http_program_output(&source),
        "True True True True True True 0 1 0\nSun, 06 Nov 1994 08:49:37 GMT\nTrue True\nTrue True"
    );
}

/// Malformed captures use an isolated renderer, never the typed handler.
#[test]
fn path_refusal_renderer_keeps_invalid_captures_out_of_handlers() {
    let source = SCRIPTED.to_owned()
        + r##"
from std.http.server import PathRefusal, RefusalResponse, RefusalError

def bad_path(why: &PathRefusal, response: &mut RefusalResponse) -> Result[(), RefusalError]:
    response.set_content_type("text/html; charset=utf-8")?
    match why:
        case PathRefusal.InvalidEscape:
            response.append(b"<p>escape</p>")?
        case PathRefusal.InvalidUtf8:
            response.append(b"<p>utf8</p>")?
    Ok(())

pub class RefusalApp:
    pub def handle(self, route: &i64, exchange: &mut Exchange[Script]) -> Result[(), HttpError]:
        exchange.send_text(200, "ordinary")

def main() -> Result[(), ServerError | AllocError | IoError]:
    mut table = std.http.server.router[i64]()?
    table.add("*", "/bad/{name}", 0)?
    table.set_fallback(Some(1))
    table.set_path_refusal(Some(bad_path), 64)
    for line in ["GET /bad/%", "GET /bad/%2", "GET /bad/%zz", "GET /bad/%ff", "GET /bad/%C3%28", "HEAD /bad/%zz", "GET /bad/good", "GET /missing/%zz"]?:
        print("CASE", line)?
        request = line.concat(" HTTP/1.1\r\nHost: x\r\n\r\n")?
        stop = CancellationToken()?
        report = std.http.server.serve_connection(Script(std.bytes.from_text(&request)?, 0, 1, False, True), &table, &RefusalApp(), &std.http.server.config(), &stop, &stop)
        print("")?
        print(counts(&report)?)?
    print("CASE framing")?
    stop = CancellationToken()?
    drop(std.http.server.serve_connection(Script(std.bytes.from_text("GET /bad/%zz HTTP/1.1\r\nHost: x\r\nContent-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\n")?, 0, 1, False, True), &table, &RefusalApp(), &std.http.server.config(), &stop, &stop))
    print("")?
    print("CASE host")?
    drop(std.http.server.serve_connection(Script(std.bytes.from_text("GET /bad/%zz HTTP/1.1\r\nHost: x@y\r\n\r\n")?, 0, 1, False, True), &table, &RefusalApp(), &std.http.server.config(), &stop, &stop))
    print("")?
    print("CASE body-limit")?
    mut limited = std.http.server.config()
    limited.limits.max_body = 0
    drop(std.http.server.serve_connection(Script(std.bytes.from_text("GET /bad/%zz HTTP/1.1\r\nHost: x\r\nContent-Length: 1\r\n\r\n")?, 0, 1, False, True), &table, &RefusalApp(), &limited, &stop, &stop))
    print("")?
    print("CASE pipeline")?
    drop(std.http.server.serve_connection(Script(std.bytes.from_text("POST /bad/%zz HTTP/1.1\r\nHost: x\r\nContent-Length: 99\r\nExpect: 100-continue\r\n\r\nGET /bad/good HTTP/1.1\r\nHost: x\r\n\r\n")?, 0, 1, False, True), &table, &RefusalApp(), &std.http.server.config(), &stop, &stop))
    print("")?
    table.set_path_refusal(Nothing, 64)
    print("CASE reset")?
    drop(std.http.server.serve_connection(Script(std.bytes.from_text("GET /bad/%zz HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")?, 0, 1, False, True), &table, &RefusalApp(), &std.http.server.config(), &stop, &stop))
    print("")?
    mut workers = routes()?
    workers.set_path_refusal(Some(bad_path), 64)
    listener = std.net.listen(&std.net.loopback_v4(0u16), 4).unwrap()
    stop.cancel()
    drop(std.http.server.serve(listener, &workers, &NetApp("worker"), &std.http.server.config(), &stop)?)
    Ok(())
"##;
    let result = support::http_program_output(&source);
    let sections: Vec<_> = result.split("CASE ").skip(1).collect();
    assert_eq!(sections.len(), 13, "{result}");
    for section in &sections[..6] {
        assert!(section.contains("HTTP/1.1 400 Bad Request|"), "{section}");
        assert!(
            section.contains("Content-Type: text/html; charset=utf-8|"),
            "{section}"
        );
        assert!(section.contains("Connection: close|"), "{section}");
        assert!(section.contains("protocol_errors=1"), "{section}");
        assert!(!section.contains("ordinary"), "{section}");
        if section.starts_with("HEAD") {
            assert!(section.contains("Content-Length: 13|"), "{section}");
            assert!(!section.contains("<p>"), "{section}");
        } else if section.contains("%ff") || section.contains("%C3%28") {
            assert!(section.contains("Content-Length: 11|"), "{section}");
            assert!(section.contains("<p>utf8</p>"), "{section}");
        } else {
            assert!(section.contains("Content-Length: 13|"), "{section}");
            assert!(section.contains("<p>escape</p>"), "{section}");
        }
    }
    for section in &sections[6..8] {
        assert!(section.contains("HTTP/1.1 200 OK|"), "{section}");
        assert!(section.contains("ordinary"), "{section}");
    }
    assert!(!sections[8].contains("<p>"), "{}", sections[8]);
    assert!(sections[8].contains("400 Bad Request"), "{}", sections[8]);
    assert!(sections[9].contains("400 Bad Request"), "{}", sections[9]);
    assert!(
        sections[10].contains("413 Content Too Large"),
        "{}",
        sections[10]
    );
    assert!(!sections[9].contains("<p>"), "{}", sections[9]);
    assert!(!sections[10].contains("<p>"), "{}", sections[10]);
    assert_eq!(
        sections[11].matches("HTTP/1.1 ").count(),
        1,
        "{}",
        sections[11]
    );
    assert!(!sections[11].contains("ordinary"), "{}", sections[11]);
    assert!(sections[12].contains("||Bad Request"), "{}", sections[12]);
    assert!(!sections[12].contains("<p>"), "{}", sections[12]);
}

#[test]
fn path_refusal_render_failures_publish_no_partial_response() {
    let source = SCRIPTED.to_owned()
        + r##"
from std.http.server import PathRefusal, RefusalResponse, RefusalError

def oversized(why: &PathRefusal, response: &mut RefusalResponse) -> Result[(), RefusalError]:
    response.append(b"abcdef")

def ignored(why: &PathRefusal, response: &mut RefusalResponse) -> Result[(), RefusalError]:
    drop(response.append(b"abcdef"))
    Ok(())

def injection(why: &PathRefusal, response: &mut RefusalResponse) -> Result[(), RefusalError]:
    response.set_content_type("text/html\r\nContent-Length: 99")

def empty(why: &PathRefusal, response: &mut RefusalResponse) -> Result[(), RefusalError]:
    response.append(b"")

def main() -> Result[(), ServerError | AllocError | IoError]:
    mut table = std.http.server.router[Route]()?
    table.add("GET", "/bad/{name}", Route.Home)?
    config = std.http.server.config()
    for kind in range(5):
        print("CASE", kind)?
        if kind == 0:
            table.set_path_refusal(Some(oversized), 5)
        elif kind == 1:
            table.set_path_refusal(Some(ignored), 5)
        elif kind == 2:
            table.set_path_refusal(Some(injection), 5)
        elif kind == 3:
            table.set_path_refusal(Some(empty), 0)
        else:
            table.set_path_refusal(Some(oversized), 100)
        mut limits = std.http.server.config()
        if kind == 4:
            limits.limits.max_body = 1
        stop = CancellationToken()?
        report = std.http.server.serve_connection(Script(std.bytes.from_text("GET /bad/%zz HTTP/1.1\r\nHost: x\r\n\r\n")?, 0, 1, False, True), &table, &App("ordinary"), &limits, &stop, &stop)
        print("")?
        print(counts(&report)?)?
    print("CASE expired")?
    table.set_path_refusal(Some(empty), 0)
    mut expires = std.http.server.config()
    expires.response_timeout = std.time.nanoseconds(1u64)
    stop = CancellationToken()?
    report = std.http.server.serve_connection(Script(std.bytes.from_text("GET /bad/%zz HTTP/1.1\r\nHost: x\r\n\r\n")?, 0, 1, False, True), &table, &App("ordinary"), &expires, &stop, &stop)
    print("")?
    print(counts(&report)?)?
    Ok(())
"##;
    let result = support::http_program_output(&source);
    let sections: Vec<_> = result.split("CASE ").skip(1).collect();
    assert_eq!(sections.len(), 6, "{result}");
    for index in [0, 1, 2, 4] {
        let section = sections[index];
        assert!(!section.contains("HTTP/1.1"), "{section}");
        assert!(section.contains("handler_errors=1"), "{section}");
    }
    assert!(
        sections[3].contains("Content-Length: 0|"),
        "{}",
        sections[3]
    );
    assert!(!sections[5].contains("HTTP/1.1"), "{}", sections[5]);
    assert!(sections[5].contains("timeouts=1"), "{}", sections[5]);
}

#[cfg(feature = "runtime-checks")]
#[test]
fn path_refusal_buffer_preserves_allocation_cancellation_and_failure_limits() {
    let module = include_str!("../stdlib/std/http/server.plenty");
    let types = &module[module.find("pub enum PathRefusal:").unwrap()
        ..module.find("# Routes map a method").unwrap()];
    let helpers = ["def decoded(", "def http_allocated[T]("]
        .iter()
        .map(|marker| {
            let start = module.find(marker).unwrap();
            module[start..].split("\n\n").next().unwrap()
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let source = format!(
        r##"import std.bytes
import std.text
import std.url
import std.time
from std.bytes import Bytes
from std.http import HttpError
from std.sync import CancellationToken
from std.time import monotonic_now, instant_at
{types}
{helpers}

def now_ns() -> u64:
    monotonic_now().saturating_duration_since(&instant_at(0u64)).as_nanoseconds()

def main() -> Result[(), AllocError | IoError]:
    token = CancellationToken()?
    mut limited = RefusalResponse(std.bytes.empty()?, "text/plain", 5, u64.MAX, token.share(), False)
    limited.append(b"123").unwrap()
    print(limited.append(b"456").is_err(), limited._body.len(), limited.check().is_err())?
    mut header = RefusalResponse(std.bytes.empty()?, "text/plain", 5, u64.MAX, token.share(), False)
    print(header.set_content_type("text/\nhtml").is_err(), header.check().is_err())?
    mut allocation = RefusalResponse(std.bytes.empty()?, "text/plain", 5, u64.MAX, token.share(), False)
    print("__test_fail_allocations_after_0__")?
    failed = match allocation.append(b"a"):
        case Err(RefusalError.Allocation(_)):
            True
        case _:
            False
    print("__test_restore_allocations__")?
    print(failed, allocation._body.len(), allocation.check().is_err())?
    expired = RefusalResponse(std.bytes.empty()?, "text/plain", 5, 0u64, token.share(), False)
    timed_out = match expired.check_at(&std.time.instant_at(0u64)):
        case Err(RefusalError.TimedOut):
            True
        case _:
            False
    print(timed_out)?
    cancelled = RefusalResponse(std.bytes.empty()?, "text/plain", 5, u64.MAX, token.share(), False)
    boundary = RefusalResponse(std.bytes.empty()?, "text/plain", 5, 100u64, token.share(), False)
    print(boundary.check_at(&std.time.instant_at(99u64)).is_ok(), boundary.check_at(&std.time.instant_at(100u64)).is_err())?
    captures = ["valid"]?
    print("__test_fail_allocations_after_0__")?
    decode_failed = match decoded(&captures):
        case Err(HttpError.Allocation(_)):
            True
        case _:
            False
    print("__test_restore_allocations__")?
    print(decode_failed)?
    token.cancel()
    stopped = match cancelled.check():
        case Err(RefusalError.Cancelled):
            True
        case _:
            False
    print(stopped)?
    Ok(())
"##
    );
    assert_eq!(
        support::http_program_output(&source),
        "True 3 True\nTrue True\nTrue 0 True\nTrue\nTrue True\nTrue\nTrue"
    );
}

#[test]
fn path_refusal_renderer_is_checked_as_worker_code() {
    let source = SCRIPTED.to_owned()
        + r##"
import std.env
from std.http.server import PathRefusal, RefusalResponse, RefusalError

def unsafe_renderer(why: &PathRefusal, response: &mut RefusalResponse) -> Result[(), RefusalError]:
    path = std.env.current_dir().unwrap()
    std.env.set_current_dir(&path).unwrap()
    Ok(())

def main() -> ():
    mut table = routes().unwrap()
    table.set_path_refusal(Some(unsafe_renderer), 64)
    stop = CancellationToken().unwrap()
    stop.cancel()
    listener = std.net.listen(&std.net.loopback_v4(0u16), 4).unwrap()
    drop(std.http.server.serve(listener, &table, &NetApp("worker"), &std.http.server.config(), &stop).unwrap())
"##;
    let error = support::check_diagnostic(&source);
    assert!(error.contains("changing the working directory"), "{error}");
    assert!(error.contains("unsafe_renderer"), "{error}");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn response_content_types_share_text_before_protocol_serialization() {
    let source = format!(
        "{SCRIPTED}{}",
        r#"
pub class Sending:
    content_type: str
    marker: str

    pub def handle(self, route: &i64, exchange: &mut Exchange[Script]) -> Result[(), IoError]:
        print(&self.marker)?
        sent = exchange.send(200, &self.content_type, b"ok")
        print("__test_restore_allocations__")?
        print(sent.is_ok())?
        Ok(())

def main() -> Result[(), ServerError | AllocError | IoError]:
    mut table = std.http.server.router[i64]()?
    table.add("GET", "/", 1)?
    stop = CancellationToken()?
    config = std.http.server.config()
    for content_type in ["a", "application/a-content-type-beyond-inline-storage"]?:
        for budget in range(40):
            marker = "__test_fail_allocations_after_".concat(str.from(budget)?)?.concat("__")?
            input = std.bytes.from_text("GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")?
            app = Sending(content_type, marker)
            drop(std.http.server.serve_connection(Script(input, 0, 1000, False, False), &table, &app, &config, &stop, &stop))
    Ok(())
"#
    );
    let output = support::http_program_output(&source);
    let results: Vec<_> = output.lines().collect();
    assert_eq!(results.len(), 80, "{output}");
    let (short, long) = results.split_at(40);
    assert_eq!(short, long, "{output}");
    assert_eq!(short[0], "False");
    assert_eq!(short[39], "True");
}
