//! `std.signal`: termination signals sent to compiled programs. See
//! book/src/design/stdlib/signal.md.
mod support;

use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

const SIGINT: i32 = 2;
const SIGTERM: i32 = 15;

unsafe extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
}

fn send(child: &Child, signal: i32) {
    // SAFETY: plain integer arguments; the child is unreaped, so its pid is ours.
    assert_eq!(unsafe { kill(child.id() as i32, signal) }, 0);
}

/// A running program and its standard output, read line by line.
struct Running {
    child: Child,
    lines: BufReader<ChildStdout>,
    _dir: tempfile::TempDir,
}

impl Running {
    fn start(source: &str) -> Running {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.plenty");
        let built = dir.path().join("built");
        let executable = dir.path().join("program");
        std::fs::write(&path, source).unwrap();
        plenty::compile_file_to_executable(&path, &built, None)
            .unwrap_or_else(|error| panic!("{source}\n{error}"));
        support::install_executable(&built, &executable);
        let mut child = Command::new(executable)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let lines = BufReader::new(child.stdout.take().unwrap());
        Running {
            child,
            lines,
            _dir: dir,
        }
    }

    fn line(&mut self) -> String {
        let mut line = String::new();
        self.lines.read_line(&mut line).unwrap();
        line.trim_end().to_owned()
    }

    /// The rest of the output and the exit status.
    fn finish(mut self) -> (String, std::process::ExitStatus) {
        let mut rest = String::new();
        self.lines.read_to_string(&mut rest).unwrap();
        let status = self.child.wait().unwrap();
        (rest.trim_end().to_owned(), status)
    }
}

/// Streams "tick" until shutdown begins, then ends with "bye". `main` prints
/// the port, the signal that stopped the server and the request count.
const DRAINING_SERVER: &str = r##"import std.bytes
import std.http
import std.http.server
import std.net
import std.ready
import std.signal
import std.time
from std.http.server import Exchange
from std.net import TcpStream
from std.sync import CancellationToken

pub enum Route:
    Stream

pub class Ticker:
    pub def handle(self, route: &Route, exchange: &mut Exchange[TcpStream]) -> Result[(), AllocError | std.bytes.BytesError | std.http.HttpError]:
        exchange.start(std.http.response(200, "OK")?)?
        tick = std.bytes.from_text("tick\n")?
        while not exchange.stopping():
            exchange.write(&tick)?
            std.time.sleep(&std.time.milliseconds(20u32))
        bye = std.bytes.from_text("bye\n")?
        exchange.write(&bye)?
        Ok(())

def main() -> Result[(), AllocError | IoError | SpawnError[()] | std.http.server.ServerError | std.net.NetError | std.ready.WaitError | std.signal.SignalError]:
    signals = std.signal.listen()?
    mut routes = std.http.server.router[Route]()?
    routes.add("GET", "/stream", Route.Stream)?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    print(listener.local_address()?.port)?
    stop = CancellationToken()?
    with spawn(std.signal.cancel_on, signals, stop.share()).drop_job()? as watcher:
        served = std.http.server.serve(listener, &routes, &Ticker(), &std.http.server.config(), &stop)
        stop.cancel()
        match watcher.join()?:
            case Some(signal):
                print(signal)?
            case Nothing:
                print("no signal")?
        print(served?.requests)?
    Ok(())
"##;

fn read_until_tick(stream: &mut impl Read, received: &mut Vec<u8>) -> io::Result<()> {
    let mut buffer = [0u8; 512];
    while !String::from_utf8_lossy(received).contains("tick") {
        let count = match stream.read(&mut buffer) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!("ended before tick: {}", String::from_utf8_lossy(received)),
            ));
        }
        received.extend_from_slice(&buffer[..count]);
    }
    Ok(())
}

#[test]
fn sigterm_drains_a_streaming_response() {
    let mut server = Running::start(DRAINING_SERVER);
    let port: u16 = server.line().parse().unwrap();
    let mut client = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    client
        .write_all(b"GET /stream HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let mut received = Vec::new();
    read_until_tick(&mut client, &mut received).unwrap();
    send(&server.child, SIGTERM);
    client.read_to_end(&mut received).unwrap();
    let text = String::from_utf8_lossy(&received);
    assert!(text.starts_with("HTTP/1.1 200 OK\r\n"), "{text}");
    assert!(text.contains("bye\n"), "{text}");
    assert!(text.ends_with("0\r\n\r\n"), "{text}");
    let (rest, status) = server.finish();
    assert!(status.success(), "{status:?}\n{rest}");
    assert_eq!(rest, "std.signal.Signal.Terminate\n1");
}

#[test]
fn sigint_reaches_a_waiting_program() {
    let mut program = Running::start(
        r##"import std.ready
import std.signal
from std.signal import Signal, SignalError
from std.sync import CancellationToken
from std.time import deadline_after, monotonic_now, seconds

def main() -> Result[(), AllocError | IoError | RangeError | SignalError | std.ready.WaitError]:
    mut signals = std.signal.listen()?
    match std.signal.listen():
        case Err(SignalError.AlreadyListening):
            print("second listen refused")?
        case _:
            print("second listen allowed")?
    print(signals.received()?)?
    print("ready")?
    never = CancellationToken()?
    deadline = deadline_after(&monotonic_now(), &seconds(10u32))?
    match signals.wait(&deadline, &never)?:
        case Signal.Interrupt:
            print("interrupt")?
        case Signal.Terminate:
            print("terminate")?
    Ok(())
"##,
    );
    assert_eq!(program.line(), "second listen refused");
    assert_eq!(program.line(), "Option[std.signal.Signal].Nothing");
    assert_eq!(program.line(), "ready");
    send(&program.child, SIGINT);
    let (rest, status) = program.finish();
    assert!(status.success(), "{status:?}\n{rest}");
    assert_eq!(rest, "interrupt");
}

#[test]
fn a_selector_reports_a_signal() {
    let mut program = Running::start(
        r##"import std.ready
import std.signal
from std.sync import CancellationToken
from std.time import deadline_after, monotonic_now, seconds

def main() -> Result[(), AllocError | IoError | RangeError | std.ready.WaitError | std.signal.SignalError]:
    mut signals = std.signal.listen()?
    mut selector = std.ready.selector(4)?
    signals.watch(&mut selector, 7u64)?
    print("ready")?
    never = CancellationToken()?
    deadline = deadline_after(&monotonic_now(), &seconds(10u32))?
    count = selector.wait(&deadline, &never)?
    print(selector.event(0)?.key)?
    print(signals.received()?)?
    print(signals.received()?)?
    signals.unwatch(&mut selector)?
    Ok(())
"##,
    );
    assert_eq!(program.line(), "ready");
    send(&program.child, SIGTERM);
    let (rest, status) = program.finish();
    assert!(status.success(), "{status:?}\n{rest}");
    assert_eq!(
        rest,
        "7\nOption[std.signal.Signal].Some(std.signal.Signal.Terminate)\nOption[std.signal.Signal].Nothing"
    );
}

/// Listens, optionally closes the handle, then sleeps without reading.
fn sleeper(close: bool) -> String {
    let closing = if close { "    signals.close()?\n" } else { "" };
    format!(
        r##"import std.ready
import std.signal
import std.time

def main() -> Result[(), IoError | std.ready.WaitError | std.signal.SignalError]:
    mut signals = std.signal.listen()?
{closing}    print("ready")?
    std.time.sleep(&std.time.seconds(20u32))
    print("slept")?
    Ok(())
"##
    )
}

fn assert_ends_by(program: Running, signal: i32) {
    let started = Instant::now();
    let (rest, status) = program.finish();
    assert_eq!(status.signal(), Some(signal), "{status:?}\n{rest}");
    assert!(started.elapsed() < Duration::from_secs(10), "{rest}");
}

#[test]
fn a_second_signal_ends_the_program_at_once() {
    let mut program = Running::start(&sleeper(false));
    assert_eq!(program.line(), "ready");
    send(&program.child, SIGTERM);
    // The first is caught and queued for a reader; the program sleeps on.
    std::thread::sleep(Duration::from_millis(200));
    assert!(program.child.try_wait().unwrap().is_none());
    send(&program.child, SIGINT);
    assert_ends_by(program, SIGINT);
}

#[test]
fn a_closed_handle_restores_the_default_action() {
    let mut program = Running::start(&sleeper(true));
    assert_eq!(program.line(), "ready");
    send(&program.child, SIGTERM);
    assert_ends_by(program, SIGTERM);
}

#[test]
fn a_program_that_does_not_listen_keeps_the_default_action() {
    let mut program = Running::start(
        r##"import std.time

def main() -> Result[(), IoError]:
    print("ready")?
    std.time.sleep(&std.time.seconds(20u32))
    Ok(())
"##,
    );
    assert_eq!(program.line(), "ready");
    send(&program.child, SIGTERM);
    assert_ends_by(program, SIGTERM);
}

#[test]
fn a_child_process_keeps_the_default_action() {
    let program = Running::start(
        r##"import std.process
import std.signal

def main() -> Result[(), IoError | std.signal.SignalError]:
    signals = std.signal.listen()?
    mut command = std.process.command("sleep")?
    command.arg("20")?
    mut child = command.spawn()?
    child.terminate()?
    print(child.wait()?.signal())?
    drop(signals)
    Ok(())
"##,
    );
    let (rest, status) = program.finish();
    assert!(status.success(), "{status:?}\n{rest}");
    assert_eq!(rest, "Option[i32].Some(15)");
}

#[test]
fn a_worker_cannot_listen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    std::fs::write(
        &path,
        r##"import std.signal

def worker() -> Result[(), std.signal.SignalError]:
    std.signal.listen()?
    Ok(())

def main() -> Result[(), SpawnError[()] | std.signal.SignalError]:
    with spawn(worker).drop_job()? as task:
        task.join()?
    Ok(())
"##,
    )
    .unwrap();
    let error = plenty::check_file(&path, None).unwrap_err().to_string();
    assert!(
        error.contains("catching signals changes how every thread's process ends"),
        "{error}"
    );
}

#[test]
fn marker_read_retries_only_interruptions_and_requires_the_marker() {
    struct ScriptedReads<'a> {
        steps: std::collections::VecDeque<io::Result<&'a [u8]>>,
    }
    impl Read for ScriptedReads<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let data = self
                .steps
                .pop_front()
                .expect("unexpected read past marker")?;
            assert!(data.len() <= buf.len());
            buf[..data.len()].copy_from_slice(data);
            Ok(data.len())
        }
    }
    let mut stream = ScriptedReads {
        steps: [
            Err(io::ErrorKind::Interrupted.into()),
            Ok(b"ti".as_slice()),
            Err(io::ErrorKind::Interrupted.into()),
            Ok(b"ck\n".as_slice()),
        ]
        .into(),
    };
    let mut received = Vec::new();
    read_until_tick(&mut stream, &mut received).unwrap();
    assert_eq!(received, b"tick\n");
    for kind in [
        io::ErrorKind::TimedOut,
        io::ErrorKind::WouldBlock,
        io::ErrorKind::ConnectionReset,
    ] {
        let mut stream = ScriptedReads {
            steps: [Ok(b"ti".as_slice()), Err(kind.into())].into(),
        };
        let mut received = Vec::new();
        assert_eq!(
            read_until_tick(&mut stream, &mut received)
                .unwrap_err()
                .kind(),
            kind
        );
        assert_eq!(received, b"ti");
    }
    let mut received = Vec::new();
    assert_eq!(
        read_until_tick(&mut b"ti".as_slice(), &mut received)
            .unwrap_err()
            .kind(),
        io::ErrorKind::UnexpectedEof
    );
}
