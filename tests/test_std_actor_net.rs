//! `std.actor.net`: listeners, connections, connect and lookup for actors,
//! over real loopback sockets. See book/src/design/stdlib/actor-net.md.
mod support;

use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

/// An echo server whose limits each test sets; see the fixture's header.
const SERVER: &str = include_str!("fixtures/actor/net_server.plenty");

/// A lookup, a refused connect and a request to a live port.
const CLIENT: &str = include_str!("fixtures/actor/net_client.plenty");

/// Replaces each `const NAME: TYPE = VALUE` line named in `settings`.
fn configured(source: &str, settings: &[(&str, &str)]) -> String {
    let mut text = source.to_string();
    for (name, value) in settings {
        let start = text
            .find(&format!("const {name}: "))
            .unwrap_or_else(|| panic!("no constant {name}"));
        let end = start + text[start..].find('\n').unwrap();
        let declaration = &text[start..end];
        let kind = declaration
            .split(": ")
            .nth(1)
            .unwrap()
            .split(" = ")
            .next()
            .unwrap();
        let replacement = format!("const {name}: {kind} = {value}");
        text.replace_range(start..end, &replacement);
    }
    text
}

struct Server {
    child: Child,
    lines: BufReader<ChildStdout>,
    port: u16,
    _dir: tempfile::TempDir,
}

impl Server {
    fn start(settings: &[(&str, &str)]) -> Server {
        let (dir, executable) = support::build_file(&configured(SERVER, settings));
        let mut child = Command::new(executable)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap());
        let mut port = String::new();
        lines.read_line(&mut port).unwrap();
        let port = port.trim().parse().unwrap();
        Server {
            child,
            lines,
            port,
            _dir: dir,
        }
    }

    fn connect(&self) -> TcpStream {
        let stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        stream
    }

    /// Waits for exit and returns the counter lines and stderr.
    fn finish(mut self) -> (String, String) {
        let mut summary = String::new();
        self.lines.read_to_string(&mut summary).unwrap();
        let mut stderr = String::new();
        self.child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .unwrap();
        let status = self.child.wait().unwrap();
        assert!(status.success(), "{status:?}: {stderr}");
        (summary, stderr)
    }
}

/// The value printed after `name` on its own line.
fn counter(summary: &str, name: &str) -> i64 {
    summary
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{name} ")))
        .unwrap_or_else(|| panic!("no {name} in {summary}"))
        .parse()
        .unwrap()
}

/// The stage ended with every heap discarded.
fn assert_clean(summary: &str, started: usize) {
    let stats = summary.lines().last().unwrap();
    assert!(
        stats.starts_with(&format!("std.actor.Stats(started={started},")),
        "{summary}"
    );
    assert!(stats.ends_with("lost=0) 0 0"), "{summary}");
}

/// Deterministic bytes that never start with `X`.
fn payload(seed: u64, length: usize) -> Vec<u8> {
    let mut state = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    let mut bytes = Vec::with_capacity(length);
    for _ in 0..length {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        bytes.push((state >> 33) as u8);
    }
    bytes[0] = b'a';
    bytes
}

/// Sends `data` while reading the echo, then returns everything read.
fn echo(mut stream: TcpStream, data: Vec<u8>) -> Vec<u8> {
    let mut writer = stream.try_clone().unwrap();
    let sender = std::thread::spawn(move || {
        for chunk in data.chunks(65536) {
            writer.write_all(chunk).unwrap();
        }
        writer.shutdown(Shutdown::Write).unwrap();
    });
    let mut received = Vec::new();
    stream.read_to_end(&mut received).unwrap();
    sender.join().unwrap();
    received
}

/// Reads until end of input; a reset counts as the end.
fn drained(stream: &mut TcpStream) -> usize {
    let mut rest = Vec::new();
    match stream.read_to_end(&mut rest) {
        Ok(count) => count,
        Err(error) => {
            assert_eq!(error.kind(), ErrorKind::ConnectionReset);
            0
        }
    }
}

/// Small buffers turn large pipelined echoes into many partial reads and
/// writes; a trapping connection ends alone.
#[test]
fn echo_with_partial_reads_and_writes_and_a_trap() {
    const GOOD: usize = 12;
    let server = Server::start(&[
        ("connections", &(GOOD + 1).to_string()),
        ("input_size", "512"),
        ("output_size", "700"),
    ]);
    let mut bad = server.connect();
    bad.write_all(b"Xboom").unwrap();
    let port = server.port;
    let clients: Vec<_> = (0..GOOD)
        .map(|index| {
            std::thread::spawn(move || {
                let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(30)))
                    .unwrap();
                let data = payload(index as u64, 1_000_000 + index * 4099);
                let received = echo(stream, data.clone());
                assert!(
                    received == data,
                    "connection {index} was not echoed exactly"
                );
            })
        })
        .collect();
    assert_eq!(drained(&mut bad), 0);
    for client in clients {
        client.join().unwrap();
    }
    let (summary, stderr) = server.finish();
    assert!(stderr.contains("unwrap on Nothing"), "{stderr}");
    assert_eq!(counter(&summary, "trapped"), 1, "{summary}");
    assert_clean(&summary, GOOD + 2);
    // Buffers, connection state and the actor's own bookkeeping.
    let used = counter(&summary, "used");
    assert!(used > 1212 && used < 4096, "{summary}");
}

/// A peer that sends without reading fills the output buffer, then the
/// input buffer, then the kernel's buffers; the server then stops reading
/// instead of buffering more. Reading afterwards gets every byte back.
#[test]
fn back_pressure_stops_reading() {
    let server = Server::start(&[
        ("connections", "1"),
        ("input_size", "4096"),
        ("output_size", "4096"),
    ]);
    let mut stream = server.connect();
    stream.set_nonblocking(true).unwrap();
    let data = payload(7, 64 << 20);
    let mut sent = 0;
    let mut quiet = Instant::now();
    // Sends until the peer has accepted nothing for half a second.
    while quiet.elapsed() < Duration::from_millis(500) {
        match stream.write(&data[sent..(sent + 65536).min(data.len())]) {
            Ok(count) => {
                sent += count;
                quiet = Instant::now();
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("{error}"),
        }
        assert!(sent < data.len(), "the server read without limit");
    }
    // Two directions of loopback socket buffers and 8 KiB in the actor.
    assert!(sent < 32 << 20, "sent {sent}");
    stream.set_nonblocking(false).unwrap();
    let received = echo(stream, data[sent..sent + (1 << 20)].to_vec());
    let mut expected = data[..sent].to_vec();
    expected.extend_from_slice(&data[sent..sent + (1 << 20)]);
    assert!(received == expected, "echo lost or reordered bytes");
    let (summary, _) = server.finish();
    assert_clean(&summary, 2);
}

/// An idle connection is closed by its deadline; a read deadline ends a
/// connection that never sends; a busy one is not affected.
#[test]
fn idle_and_read_deadlines_close_connections() {
    let server = Server::start(&[("connections", "2"), ("idle_ms", "200u32")]);
    let started = Instant::now();
    let mut idle = server.connect();
    let busy = server.connect();
    let sender = std::thread::spawn(move || {
        // Each byte arrives within the idle span, for longer than it.
        let data = payload(3, 40);
        let mut writer = busy.try_clone().unwrap();
        for byte in &data {
            writer.write_all(std::slice::from_ref(byte)).unwrap();
            std::thread::sleep(Duration::from_millis(20));
        }
        writer.shutdown(Shutdown::Write).unwrap();
        let mut received = Vec::new();
        let mut reader = busy;
        reader.read_to_end(&mut received).unwrap();
        assert_eq!(received, data);
    });
    assert_eq!(drained(&mut idle), 0);
    let waited = started.elapsed();
    assert!(waited >= Duration::from_millis(190), "{waited:?}");
    sender.join().unwrap();
    let (summary, _) = server.finish();
    assert_eq!(counter(&summary, "idle"), 1, "{summary}");
    assert_clean(&summary, 3);

    let server = Server::start(&[("connections", "1"), ("read_ms", "150u32")]);
    let mut silent = server.connect();
    assert_eq!(drained(&mut silent), 0);
    let (summary, _) = server.finish();
    assert_eq!(counter(&summary, "read"), 1, "{summary}");
}

/// A peer that stops reading stalls the output; the stall deadline ends it.
#[test]
fn stalled_output_is_closed() {
    let server = Server::start(&[
        ("connections", "1"),
        ("input_size", "1024"),
        ("output_size", "1024"),
        ("stall_ms", "200u32"),
    ]);
    let mut stream = server.connect();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let data = payload(5, 32 << 20);
    // Writes fail once the server closes the connection.
    let _ = stream.write_all(&data);
    let (summary, _) = server.finish();
    assert_eq!(counter(&summary, "stalled"), 1, "{summary}");
    assert_clean(&summary, 2);
}

/// At capacity a refusing listener closes new connections at once, and
/// admits again when a connection ends.
#[test]
fn refuses_at_capacity() {
    let server = Server::start(&[("connections", "3"), ("capacity", "2"), ("refuse", "True")]);
    let mut held: Vec<TcpStream> = (0..2).map(|_| server.connect()).collect();
    // Admission is in order, so both held connections are being served.
    for (index, stream) in held.iter_mut().enumerate() {
        stream.write_all(&[b'a' + index as u8]).unwrap();
        let mut byte = [0u8];
        stream.read_exact(&mut byte).unwrap();
    }
    let mut refused = server.connect();
    assert_eq!(drained(&mut refused), 0);
    let first = held.remove(0);
    assert_eq!(echo(first, b"done".to_vec()), b"done");
    // The ended connection's place is free again; the earlier read only
    // shows the refusal, so retry until a new connection is admitted.
    let mut admitted = None;
    for _ in 0..50 {
        let mut stream = server.connect();
        let mut reply = [0u8; 5];
        if stream.write_all(b"again").is_ok() && stream.read_exact(&mut reply).is_ok() {
            admitted = Some(stream);
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let admitted = admitted.expect("no connection admitted after one ended");
    drop(admitted);
    for stream in held {
        drop(stream);
    }
    let (summary, _) = server.finish();
    assert!(counter(&summary, "refused") >= 1, "{summary}");
}

/// A pausing listener leaves arrivals in the backlog until a place frees.
#[test]
fn pauses_at_capacity() {
    let server = Server::start(&[("connections", "2"), ("capacity", "1")]);
    let mut first = server.connect();
    first.write_all(b"a").unwrap();
    let mut byte = [0u8];
    first.read_exact(&mut byte).unwrap();
    let mut second = server.connect();
    second.write_all(b"b").unwrap();
    second
        .set_read_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    let error = second.read_exact(&mut byte).unwrap_err();
    assert!(
        matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut),
        "{error}"
    );
    assert_eq!(echo(first, Vec::new()), b"");
    second
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    second.read_exact(&mut byte).unwrap();
    assert_eq!(&byte, b"b");
    assert_eq!(echo(second, Vec::new()), b"");
    let (summary, _) = server.finish();
    assert_eq!(counter(&summary, "refused"), 0, "{summary}");
    assert_clean(&summary, 3);
}

/// Lookup, a refused connect, and a request over a dialled connection.
#[test]
fn dial_and_lookup_from_an_actor() {
    let closed = TcpListener::bind("127.0.0.1:0").unwrap();
    let closed_port = closed.local_addr().unwrap().port();
    drop(closed);
    let open = TcpListener::bind("127.0.0.1:0").unwrap();
    let open_port = open.local_addr().unwrap().port();
    let peer = std::thread::spawn(move || {
        let (mut stream, _) = open.accept().unwrap();
        let mut request = Vec::new();
        stream.read_to_end(&mut request).unwrap();
        assert_eq!(request, b"hello");
        stream.write_all(b"hi there").unwrap();
    });
    let source = configured(
        CLIENT,
        &[
            ("closed_port", &format!("{closed_port}u16")),
            ("open_port", &format!("{open_port}u16")),
        ],
    );
    let (_dir, executable) = support::build_file(&source);
    let output = Command::new(executable).output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    peer.join().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines[..3],
        [
            "resolved True 80",
            "closed port std.net.NetError.Refused",
            "reply hi there"
        ],
        "{stdout}"
    );
    // Readiness can arrive in more or fewer messages.
    assert!(
        lines[3]
            .starts_with("std.actor.Stats(started=1, stopped=1, failed=0, trapped=0, restarted=0,")
            && lines[3].ends_with("lost=0) 0"),
        "{stdout}"
    );
}
