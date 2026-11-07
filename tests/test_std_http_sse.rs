//! `std.http.sse`: Server-Sent Events over `std.http.server`, checked byte
//! for byte against raw sockets on 127.0.0.1, with `Exchange.detach` under
//! it: field encoding and line splitting, streams that outlive the
//! response deadline, client disconnects seen by `wait` and by `send`,
//! shutdown, HEAD requests and refused heads.
mod support;

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// A running Plenty program whose first output line is its port.
struct Running {
    child: Child,
    lines: BufReader<std::process::ChildStdout>,
    port: u16,
    _dir: tempfile::TempDir,
}

impl Running {
    fn start(source: &str) -> Running {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.plenty");
        let executable = dir.path().join("program");
        std::fs::write(&path, source).unwrap();
        plenty::compile_file_to_executable(&path, &executable, None)
            .unwrap_or_else(|error| panic!("{source}\n{error}"));
        let mut child = Command::new(executable)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap());
        let mut first = String::new();
        lines.read_line(&mut first).unwrap();
        let port = first
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("port line {first:?}"));
        Running {
            child,
            lines,
            port,
            _dir: dir,
        }
    }

    fn line(&mut self) -> String {
        let mut line = String::new();
        self.lines.read_line(&mut line).unwrap();
        line.trim_end().to_owned()
    }

    fn finish(mut self) -> String {
        let mut rest = String::new();
        self.lines.read_to_string(&mut rest).unwrap();
        let status = self.child.wait().unwrap();
        assert!(status.success(), "status {status:?}\n{rest}");
        rest.trim_end().to_owned()
    }
}

fn connect(port: u16) -> TcpStream {
    let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream
}

fn read_head(stream: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut one = [0u8];
    while !out.ends_with(b"\r\n\r\n") {
        // read_exact retries EINTR while preserving the socket's configured timeout.
        stream.read_exact(&mut one)?;
        out.push(one[0]);
    }
    Ok(out)
}

/// Reads a complete response head, a byte at a time, with CRLFs shown as `|`.
fn head(stream: &mut TcpStream) -> String {
    support::without_http_dates(
        &String::from_utf8(read_head(stream).unwrap())
            .unwrap()
            .replace("\r\n", "|"),
    )
}

#[test]
fn response_head_retries_interrupted_reads_without_consuming_the_body() {
    struct InterruptedReads<'a> {
        remaining: &'a [u8],
        interrupt: bool,
    }

    impl Read for InterruptedReads<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.interrupt = !self.interrupt;
            if self.interrupt {
                Err(io::ErrorKind::Interrupted.into())
            } else {
                self.remaining.read(buf)
            }
        }
    }

    let expected = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n";
    let response = [expected.as_slice(), b"data: one\n\n"].concat();
    let mut stream = InterruptedReads {
        remaining: &response,
        interrupt: false,
    };
    assert_eq!(read_head(&mut stream).unwrap(), expected);
    assert_eq!(stream.remaining, b"data: one\n\n");
}

#[test]
fn response_head_rejects_truncation_and_preserves_timeout_errors() {
    assert_eq!(
        read_head(&mut b"HTTP/1.1 200 OK\r\n".as_slice())
            .unwrap_err()
            .kind(),
        io::ErrorKind::UnexpectedEof
    );

    struct Timeout;
    impl Read for Timeout {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::ErrorKind::TimedOut.into())
        }
    }
    assert_eq!(
        read_head(&mut Timeout).unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
}

fn rest(stream: &mut TcpStream) -> String {
    let mut out = Vec::new();
    stream.read_to_end(&mut out).unwrap();
    String::from_utf8(out).unwrap()
}

fn get(port: u16, path: &str, extra: &str) -> TcpStream {
    let mut stream = connect(port);
    write!(stream, "GET {path} HTTP/1.1\r\nHost: x\r\n{extra}\r\n").unwrap();
    stream
}

const SERVER: &str = r##"import std.bytes
import std.time
import std.http
import std.http.server
import std.http.sse as sse
import std.net
from std.http.server import Exchange
from std.http.sse import EventStream, SseError
from std.net import TcpStream
from std.sync import CancellationToken
from std.time import Deadline

pub enum Route:
    Events
    Watch
    Push
    Wait
    Custom
    Stop

type Problem = SseError | std.bytes.BytesError | std.http.HttpError | RangeError | AllocError | IoError

def later() -> Result[Deadline, RangeError]:
    Ok(std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(10u32))?)

def pause() -> Result[Deadline, RangeError]:
    Ok(std.time.deadline_after(&std.time.monotonic_now(), &std.time.milliseconds(100u32))?)

def outcome(result: Result[(), SseError]) -> Result[(), IoError]:
    match result:
        case Ok(_):
            print("ok")
        case Err(error):
            print(error)

pub class App:
    pub stop: CancellationToken

    pub def handle(self, route: &Route, exchange: &mut Exchange[TcpStream]) -> Result[(), Problem]:
        mut wait_cancel = CancellationToken().unwrap()
        match route:
            case Route.Events:
                mut events = sse.accept(exchange)?
                print(events.last_event_id)?
                events.retry(&std.time.milliseconds(1500u32), &later()?, &wait_cancel)?
                events.send("one", &later()?, &wait_cancel)?
                events.wait(&pause()?, &wait_cancel)?
                events.send("a\nb\r\nc\rd\n", &later()?, &wait_cancel, event="update", id=Some("7"))?
                events.wait(&pause()?, &wait_cancel)?
                events.send("héllo", &later()?, &wait_cancel, id=Some(""))?
                events.wait(&pause()?, &wait_cancel)?
                data = std.bytes.from_text("xxbytesxx")?
                events.send_bytes(&data[2:7], &later()?, &wait_cancel)?
                events.comment("keep\nalive", &later()?, &wait_cancel)?
                outcome(events.send("x", &later()?, &wait_cancel, event="bad\nname"))?
                nul = std.bytes.from_list(&[97u8, 0u8, 98u8]?)?.to_text()?
                outcome(events.send("x", &later()?, &wait_cancel, id=Some(nul)))?
                bad = std.bytes.from_list(&[255u8, 254u8]?)?
                outcome(events.send_bytes(&bad, &later()?, &wait_cancel))?
                outcome(events.send_bytes(&bad[1:], &later()?, &wait_cancel))?
                wait_cancel.cancel()
                outcome(events.retry(&std.time.milliseconds(1u32), &later()?, &wait_cancel))?
                wait_cancel = CancellationToken().unwrap()
                events.wait(&pause()?, &wait_cancel)?
                events.send("last", &later()?, &wait_cancel)?
                events.close()
                outcome(events.send("after", &later()?, &wait_cancel))?
            case Route.Watch:
                match sse.accept(exchange):
                    case Ok(stream):
                        mut events = stream
                        outcome(events.wait(&later()?, &wait_cancel))?
                        print(events.is_open())?
                    case Err(error):
                        print(error)?
                        if exchange.status() == 0:
                            exchange.send_text(400, "no stream\n")?
            case Route.Push:
                mut events = sse.accept(exchange)?
                for index in range(2000):
                    match events.send("tick", &later()?, &wait_cancel):
                        case Ok(_):
                            std.time.sleep(&std.time.milliseconds(5u32))
                        case Err(error):
                            print(error)?
                            return Ok(())
                print("never failed")?
            case Route.Wait:
                mut events = sse.accept(exchange)?
                match events.wait(&later()?, &wait_cancel):
                    case Err(SseError.Stopping):
                        print("stopping")?
                        outcome(events.wait(&later()?, &wait_cancel))?
                        events.send("bye", &later()?, &wait_cancel, event="shutdown")?
                    case _:
                        print("not stopped")?
            case Route.Custom:
                mut wrong = std.http.response(200, "OK")?
                wrong.add("Content-Length", "5")?
                match sse.accept_with(exchange, wrong):
                    case Ok(_):
                        print("accepted")?
                    case Err(error):
                        print(error)?
                match sse.accept_with(exchange, std.http.response(204, "No Content")?):
                    case Ok(_):
                        print("accepted")?
                    case Err(error):
                        print(error)?
                mut cors = std.http.response(200, "OK")?
                cors.add("Access-Control-Allow-Origin", "*")?
                mut events = sse.accept_with(exchange, cors)?
                events.send("hi", &later()?, &wait_cancel)?
            case Route.Stop:
                exchange.send_text(200, "stopping")?
                self.stop.cancel()
        Ok(())

def main() -> Result[(), std.net.NetError | std.http.server.ServerError | AllocError | IoError]:
    mut table = std.http.server.router[Route]()?
    table.add("GET", "/events", Route.Events)?
    table.add("GET", "/watch", Route.Watch)?
    table.add("POST", "/watch", Route.Watch)?
    table.add("GET", "/push", Route.Push)?
    table.add("GET", "/wait", Route.Wait)?
    table.add("GET", "/custom", Route.Custom)?
    table.add("GET", "/stop", Route.Stop)?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    print(listener.local_address()?.port)?
    stop = CancellationToken()?
    mut config = std.http.server.config()
    # Shorter than the /events stream, which detach takes out of its reach.
    config.response_timeout = std.time.milliseconds(200u32)
    report = std.http.server.serve(listener, &table, &App(stop.share()), &config, &stop)?
    print("handler_errors=".concat(&str.from(report.handler_errors)?)?)?
    Ok(())
"##;

const SSE_HEAD: &str =
    "HTTP/1.1 200 OK|Content-Type: text/event-stream|Cache-Control: no-cache|Connection: close||";

/// Fields are encoded as the WHATWG event-stream format reads them: one
/// `data` line per line of data whatever its line ending, an `event` and
/// `id` before the data, comments and `retry` on their own. Invalid names,
/// ids, text, ranges and retry times are refused without writing anything.
/// The stream outlives the server's 200 ms response deadline. A HEAD
/// request gets the head alone; heads that would frame a body are refused
/// before the exchange is answered.
#[test]
fn events_are_encoded_and_outlive_the_response_deadline() {
    let mut server = Running::start(SERVER);
    let port = server.port;

    let mut stream = get(port, "/events", "Last-Event-ID: 41\r\n");
    let started = Instant::now();
    assert_eq!(head(&mut stream), SSE_HEAD);
    assert_eq!(
        rest(&mut stream),
        "retry: 1500\n\n\
         data: one\n\n\
         event: update\nid: 7\ndata: a\ndata: b\ndata: c\ndata: d\ndata: \n\n\
         id: \ndata: héllo\n\n\
         data: bytes\n\n\
         : keep\n: alive\n\
         data: last\n\n"
    );
    assert!(started.elapsed() >= Duration::from_millis(400));
    assert_eq!(server.line(), "Option[str].Some(\"41\")");
    for error in [
        "InvalidField",
        "InvalidField",
        "InvalidText",
        "InvalidText",
        "Net(std.net.NetError.Cancelled)",
        "Closed",
    ] {
        assert_eq!(server.line(), format!("std.http.sse.SseError.{error}"));
    }

    // HEAD answers with the head alone, with ordinary framing.
    let mut stream = connect(port);
    stream
        .write_all(b"HEAD /watch HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .unwrap();
    assert_eq!(
        head(&mut stream),
        "HTTP/1.1 200 OK|Content-Type: text/event-stream|Cache-Control: no-cache|Transfer-Encoding: chunked|Connection: close||"
    );
    assert_eq!(rest(&mut stream), "");
    assert_eq!(server.line(), "std.http.sse.SseError.Closed");

    // An unread request body cannot be left behind.
    let mut stream = connect(port);
    stream
        .write_all(b"POST /watch HTTP/1.1\r\nHost: x\r\nContent-Length: 3\r\n\r\nabc")
        .unwrap();
    assert!(head(&mut stream).starts_with("HTTP/1.1 400 Bad Request|"));
    assert_eq!(
        server.line(),
        "std.http.sse.SseError.Http(std.http.HttpError.Unfinished)"
    );

    // Refused heads leave the exchange unanswered; the caller's fields stay.
    let mut stream = get(port, "/custom", "");
    assert_eq!(
        head(&mut stream),
        "HTTP/1.1 200 OK|Access-Control-Allow-Origin: *|Content-Type: text/event-stream|Cache-Control: no-cache|Connection: close||"
    );
    assert_eq!(rest(&mut stream), "data: hi\n\n");
    assert_eq!(
        server.line(),
        "std.http.sse.SseError.Http(std.http.HttpError.ConflictingFraming)"
    );
    assert_eq!(
        server.line(),
        "std.http.sse.SseError.Http(std.http.HttpError.InvalidStatusLine)"
    );

    get(port, "/stop", "");
    assert_eq!(server.finish(), "handler_errors=0");
}

/// A client that closes its connection ends `wait` at once and makes a
/// later `send` fail, both with Disconnected. Server shutdown ends `wait`
/// with Stopping and leaves the stream open for a last event.
#[test]
fn disconnects_and_shutdown_end_waits() {
    let mut server = Running::start(SERVER);
    let port = server.port;

    let mut watching = get(port, "/watch", "");
    assert_eq!(head(&mut watching), SSE_HEAD);
    let started = Instant::now();
    drop(watching);
    assert_eq!(server.line(), "std.http.sse.SseError.Disconnected");
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(server.line(), "False");

    let mut pushing = get(port, "/push", "");
    assert_eq!(head(&mut pushing), SSE_HEAD);
    let mut first = [0u8; 12];
    pushing.read_exact(&mut first).unwrap();
    assert_eq!(&first, b"data: tick\n\n");
    drop(pushing);
    assert_eq!(server.line(), "std.http.sse.SseError.Disconnected");

    let mut waiting = get(port, "/wait", "");
    assert_eq!(head(&mut waiting), SSE_HEAD);
    get(port, "/stop", "");
    assert_eq!(rest(&mut waiting), "event: shutdown\ndata: bye\n\n");
    assert_eq!(server.line(), "stopping");
    assert_eq!(server.line(), "std.http.sse.SseError.Stopping");
    assert_eq!(server.finish(), "handler_errors=0");
}

fn sse_boundary_output(source: &str, optimize: bool) -> String {
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
    let output = Command::new(executable).output().unwrap();
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
fn sse_line_positions_cover_representation_limit() {
    let module = include_str!("../stdlib/std/http/sse.plenty");
    let start = module.find("def break_positions(").unwrap();
    let helper = module[start..].split("\n\n").next().unwrap();
    let source = format!(
        r#"{helper}

def main() -> Result[(), IoError]:
    print("__test_begin_no_allocations__")?
    end, next = break_positions(9223372036854775806, 0)
    last = end == 9223372036854775806 and next == i64.MAX
    far_end, far_next = break_positions(0, 9223372036854775806)
    far = far_end == 9223372036854775806 and far_next == i64.MAX
    first_end, first_next = break_positions(0, 0)
    first = first_end == 0 and first_next == 1
    print("__test_end_no_allocations__")?
    print(last, far, first)?
    Ok(())
"#
    );
    for optimize in [false, true] {
        assert_eq!(sse_boundary_output(&source, optimize), "True True True");
    }
}

#[test]
fn sse_partial_writes_keep_exact_fields_and_close_on_cancellation() {
    let source = format!(
        "{}\n{}",
        include_str!("../stdlib/std/http/sse.plenty"),
        r#"
class Capture:
    output: Bytes
    caller: CancellationToken
    fail: bool
    closed: bool

    pub def read_before(self: &mut Capture, buffer: &mut Bytes, start: i64, stop: i64, deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, NetError]:
        Err(NetError.TimedOut)

    pub def write_before(self: &mut Capture, source: &[u8], deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, NetError]:
        if self.fail and self.output.len() > 0:
            self.caller.cancel()
            return Err(NetError.TimedOut)
        mut count = len(source)
        if count > 3:
            count = 3
        match self.output.extend(&source[:count]):
            case Ok(_):
                Ok(count)
            case Err(error):
                Err(NetError.Stream(StreamError.Allocation(error)))

    pub def shutdown(self: &mut Capture, direction: Shutdown) -> Result[(), NetError]:
        self.closed = True
        Ok(())

def main() -> Result[(), SseError | AllocError | std.bytes.BytesError | IoError | RangeError]:
    never = CancellationToken()?
    caller = CancellationToken()?
    deadline = deadline_after(&monotonic_now(), &milliseconds(1000u32))?
    mut stream = EventStream(Nothing, Some(Capture(std.bytes.empty()?, caller.share(), False, False)), std.bytes.empty()?, std.bytes.empty()?, never.share(), never.share(), True)
    stream.send_bytes(b"a\r\nb", &deadline, &caller)?
    stream.retry(&std.time.nanoseconds(u64.MAX), &deadline, &caller)?
    invalid = match stream.send("x", &deadline, &caller, event="bad\n"):
        case Err(SseError.InvalidField):
            True
        case _:
            False
    match &stream._stream:
        case Some(capture):
            print(capture.output.to_text()? == "data: a\ndata: b\n\nretry: 18446744073710\n\n", invalid, not capture.closed)?
        case Nothing:
            print("missing stream")?
    active = CancellationToken()?
    mut interrupted = EventStream(Nothing, Some(Capture(std.bytes.empty()?, active.share(), True, False)), std.bytes.empty()?, std.bytes.empty()?, never.share(), never.share(), True)
    cancelled = match interrupted.send("hello", &deadline, &active):
        case Err(SseError.Net(NetError.Cancelled)):
            True
        case _:
            False
    match &interrupted._stream:
        case Some(capture):
            print(cancelled, capture.output.to_text()? == "dat", capture.closed)?
        case Nothing:
            print("missing interrupted stream")?
    print(not interrupted.is_open())?
    Ok(())
"#
    );
    for optimize in [false, true] {
        assert_eq!(
            sse_boundary_output(&source, optimize),
            "True True True\nTrue True True\nTrue"
        );
    }
}
