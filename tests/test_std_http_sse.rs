//! `std.http.sse`: Server-Sent Events over `std.http.server`, checked byte
//! for byte against raw sockets on 127.0.0.1, with `Exchange.detach` under
//! it: field encoding and line splitting, streams that outlive the
//! response deadline, client disconnects seen by `wait` and by `send`,
//! shutdown, HEAD requests and refused heads.
use std::io::{BufRead, BufReader, Read, Write};
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

/// Reads a response head, a byte at a time, with CRLFs shown as `|`.
fn head(stream: &mut TcpStream) -> String {
    let mut out = Vec::new();
    let mut one = [0u8];
    while !out.ends_with(b"\r\n\r\n") {
        if stream.read(&mut one).unwrap() == 0 {
            break;
        }
        out.push(one[0]);
    }
    String::from_utf8(out).unwrap().replace("\r\n", "|")
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
import std.http
import std.http.server
import std.http.sse as sse
import std.net
import std.time
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
        match route:
            case Route.Events:
                mut events = sse.accept(exchange)?
                print(events.last_event_id)?
                events.retry(1500, &later()?)?
                events.send("one", &later()?)?
                events.wait(&pause()?)?
                events.send("a\nb\r\nc\rd\n", &later()?, event="update", id=Some("7"))?
                events.wait(&pause()?)?
                events.send("héllo", &later()?, id=Some(""))?
                events.wait(&pause()?)?
                data = std.bytes.from_text("xxbytesxx")?
                events.send_bytes(&data, 2, 7, &later()?)?
                events.comment("keep\nalive", &later()?)?
                outcome(events.send("x", &later()?, event="bad\nname"))?
                nul = std.bytes.from_list(&[97u8, 0u8, 98u8]?)?.to_text()?
                outcome(events.send("x", &later()?, id=Some(nul)))?
                bad = std.bytes.from_list(&[255u8, 254u8]?)?
                outcome(events.send_bytes(&bad, 0, 2, &later()?))?
                outcome(events.send_bytes(&data, 5, 3, &later()?))?
                outcome(events.retry(-1, &later()?))?
                events.wait(&pause()?)?
                events.send("last", &later()?)?
                events.close()
                outcome(events.send("after", &later()?))?
            case Route.Watch:
                match sse.accept(exchange):
                    case Ok(stream):
                        mut events = stream
                        outcome(events.wait(&later()?))?
                        print(events.is_open())?
                    case Err(error):
                        print(error)?
                        if exchange.status() == 0:
                            exchange.send_text(400, "no stream\n")?
            case Route.Push:
                mut events = sse.accept(exchange)?
                for index in range(2000):
                    match events.send("tick", &later()?):
                        case Ok(_):
                            std.time.sleep(&std.time.milliseconds(5u32))
                        case Err(error):
                            print(error)?
                            return Ok(())
                print("never failed")?
            case Route.Wait:
                mut events = sse.accept(exchange)?
                match events.wait(&later()?):
                    case Err(SseError.Stopping):
                        print("stopping")?
                        outcome(events.wait(&later()?))?
                        events.send("bye", &later()?, event="shutdown")?
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
                events.send("hi", &later()?)?
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
        "OutOfRange",
        "OutOfRange",
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
