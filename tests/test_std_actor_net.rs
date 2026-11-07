//! `std.actor.net`: listeners, connections, connect and lookup for actors,
//! over real loopback sockets. See book/src/design/stdlib/actor-net.md.
mod support;

use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddrV4, TcpListener, TcpStream};
use std::os::fd::OwnedFd;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
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
    // Keep a generous scheduling margin, and make actual echo progress rather
    // than assuming a 20-ms sleep wakes before a 200-ms idle timeout (#672).
    let idle_span = Duration::from_secs(2);
    let server = Server::start(&[("connections", "2"), ("idle_ms", "2000u32")]);
    let mut idle = server.connect();
    let started = Instant::now();
    let mut busy = server.connect();
    busy.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    busy.set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    busy.set_nodelay(true).unwrap();
    let sender = std::thread::spawn(move || -> std::io::Result<usize> {
        let data = payload(3, 1024);
        let mut received = vec![0; data.len()];
        let mut echoed = 0;
        let until = Instant::now() + idle_span * 2;
        while Instant::now() < until {
            busy.write_all(&data)?;
            busy.read_exact(&mut received)?;
            if received != data {
                return Err(std::io::Error::other("echo lost or reordered bytes"));
            }
            echoed += received.len();
        }
        busy.shutdown(Shutdown::Write)?;
        received.clear();
        busy.read_to_end(&mut received)?;
        if !received.is_empty() {
            return Err(std::io::Error::other("unexpected trailing echo bytes"));
        }
        Ok(echoed)
    });
    // Gather server accounting even if the busy peer is reset. Previously a
    // sender panic hid whether the server reported an extra idle expiration.
    let mut idle_received = Vec::new();
    let idle_result = idle.read_to_end(&mut idle_received);
    let waited = started.elapsed();
    drop(idle);
    let busy_result = sender.join();
    let (summary, stderr) = server.finish();
    assert_eq!(
        idle_result.unwrap_or_else(|error| panic!("{error}; {summary}; {stderr}")),
        0
    );
    assert!(
        waited >= idle_span - Duration::from_millis(100),
        "{waited:?}; {summary}"
    );
    let echoed = busy_result
        .expect("busy client thread panicked")
        .unwrap_or_else(|error| panic!("busy client: {error}; {summary}; {stderr}"));
    assert!(echoed > 0, "busy client made no progress; {summary}");
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

const PEER_POLL: Duration = Duration::from_millis(10);

struct DialPeer {
    stop: Sender<()>,
    thread: Option<std::thread::JoinHandle<std::io::Result<()>>>,
}

impl DialPeer {
    fn start(listener: TcpListener, limit: Duration) -> Self {
        let (stop, cancelled) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            serve_dial_peer(listener, &cancelled, Instant::now() + limit)
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }

    fn finish(mut self) {
        self.thread.take().unwrap().join().unwrap().unwrap();
    }
}

impl Drop for DialPeer {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn peer_remaining(cancelled: &Receiver<()>, deadline: Instant) -> std::io::Result<Duration> {
    if !matches!(cancelled.try_recv(), Err(mpsc::TryRecvError::Empty)) {
        return Err(std::io::Error::new(
            ErrorKind::Interrupted,
            "peer cancelled",
        ));
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(std::io::Error::new(ErrorKind::TimedOut, "peer deadline"));
    }
    Ok(remaining.min(PEER_POLL))
}

fn serve_dial_peer(
    listener: TcpListener,
    cancelled: &Receiver<()>,
    deadline: Instant,
) -> std::io::Result<()> {
    listener.set_nonblocking(true)?;
    let mut stream = loop {
        let wait = peer_remaining(cancelled, deadline)?;
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                match cancelled.recv_timeout(wait) {
                    Err(RecvTimeoutError::Timeout) => (),
                    _ => {
                        return Err(std::io::Error::new(
                            ErrorKind::Interrupted,
                            "peer cancelled",
                        ));
                    }
                }
            }
            Err(error) => return Err(error),
        }
    };
    // An accepted stream may inherit nonblocking mode on some platforms.
    stream.set_nonblocking(false)?;
    let mut request = Vec::new();
    let mut buffer = [0; 6];
    loop {
        stream.set_read_timeout(Some(peer_remaining(cancelled, deadline)?))?;
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                request.extend_from_slice(&buffer[..count]);
                if request.len() > 5 {
                    return Err(std::io::Error::other("oversized peer request"));
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::Interrupted | ErrorKind::WouldBlock | ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(error) => return Err(error),
        }
    }
    if request != b"hello" {
        return Err(std::io::Error::other(format!(
            "unexpected peer request: {request:?}"
        )));
    }
    // The reply is tiny, but retain the same absolute deadline for writes.
    let mut reply = &b"hi there"[..];
    while !reply.is_empty() {
        stream.set_write_timeout(Some(peer_remaining(cancelled, deadline)?))?;
        match stream.write(reply) {
            Ok(0) => return Err(std::io::Error::new(ErrorKind::WriteZero, "peer reply")),
            Ok(count) => reply = &reply[count..],
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::Interrupted | ErrorKind::WouldBlock | ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[test]
fn dial_peer_deadline_bounds_missing_connection_and_missing_eof() {
    for connect in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let _stream = connect.then(|| TcpStream::connect(listener.local_addr().unwrap()).unwrap());
        let (_stop, cancelled) = mpsc::channel();
        let error = serve_dial_peer(
            listener,
            &cancelled,
            Instant::now() + Duration::from_millis(30),
        )
        .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::TimedOut);
    }
}

#[test]
fn dial_peer_cancels_on_early_child_exit() {
    for connect in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let _stream = connect.then(|| TcpStream::connect(listener.local_addr().unwrap()).unwrap());
        let peer = DialPeer::start(listener, Duration::from_secs(30));
        let started = Instant::now();
        drop(peer);
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}

/// Reserve a TCP port without listening so dials fail without a port-reuse gap.
fn refused_port() -> (OwnedFd, SocketAddrV4) {
    use rustix::net::{AddressFamily, SocketFlags, SocketType, bind, getsockname, socket_with};

    // Darwin requires setting close-on-exec on the descriptor after socket().
    #[cfg(target_vendor = "apple")]
    let flags = SocketFlags::empty();
    #[cfg(not(target_vendor = "apple"))]
    let flags = SocketFlags::CLOEXEC;
    let socket = socket_with(AddressFamily::INET, SocketType::STREAM, flags, None).unwrap();
    #[cfg(target_vendor = "apple")]
    rustix::io::fcntl_setfd(&socket, rustix::io::FdFlags::CLOEXEC).unwrap();
    bind(&socket, &SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
    let address = SocketAddrV4::try_from(getsockname(&socket).unwrap()).unwrap();
    (socket, address)
}

#[test]
fn refused_port_stays_reserved_while_refusing_connections() {
    let (reservation, address) = refused_port();
    assert!(
        rustix::io::fcntl_getfd(&reservation)
            .unwrap()
            .contains(rustix::io::FdFlags::CLOEXEC)
    );
    for _ in 0..10 {
        let error =
            TcpStream::connect_timeout(&address.into(), Duration::from_secs(1)).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::ConnectionRefused);
        let error = TcpListener::bind(address).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::AddrInUse);
    }
}

/// Lookup, a refused connect, and a request over a dialled connection.
#[test]
fn dial_and_lookup_from_an_actor() {
    let (closed, closed_address) = refused_port();
    let closed_port = closed_address.port();
    let open = TcpListener::bind("127.0.0.1:0").unwrap();
    let open_port = open.local_addr().unwrap().port();
    let source = configured(
        CLIENT,
        &[
            ("closed_port", &format!("{closed_port}u16")),
            ("open_port", &format!("{open_port}u16")),
        ],
    );
    let (_dir, executable) = support::build_file(&source);
    // Keep the non-listening socket bound until the child exits: parallel tests
    // must not turn the refused dial into a successful connection (issue #848).
    let peer = DialPeer::start(open, Duration::from_secs(40));
    let output =
        support::output_within(&mut Command::new(executable), b"", Duration::from_secs(35));
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{}: {stdout}\n{stderr}",
        output.status
    );
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 4, "{stdout}\n{stderr}");
    assert_eq!(
        lines[..3],
        [
            "resolved True 80",
            "closed port std.net.NetError.Refused",
            "reply hi there"
        ],
        "{stdout}\n{stderr}"
    );
    // Readiness can arrive in more or fewer messages.
    assert!(
        lines[3]
            .starts_with("std.actor.Stats(started=1, stopped=1, failed=0, trapped=0, restarted=0,")
            && lines[3].ends_with("lost=0) 0"),
        "{stdout}\n{stderr}"
    );
    peer.finish();
    drop(closed);
}

#[test]
fn deadline_and_output_room_helpers_accept_representation_boundaries() {
    let net = include_str!("../stdlib/std/actor/net.plenty");
    let later = net
        .split("def later(")
        .nth(1)
        .unwrap()
        .split("# Buffer sizes")
        .next()
        .unwrap();
    let output_room = &net[net.find("def output_room(").unwrap()..];
    let source = format!(
        r#"def later({later}
{output_room}
def main() -> Result[(), IoError]:
    print("__test_begin_no_allocations__")?
    a = later(0u64, u64.MAX)
    b = later(u64.MAX, 0u64)
    c = later(u64.MAX, 1u64)
    d = output_room(i64.MAX, 9223372036854775806)
    e = output_room(i64.MAX, 0)
    f = count_refusal(9223372036854775806)
    g = count_refusal(i64.MAX)
    print("__test_end_no_allocations__")?
    print(a, b, c, d, e, f, g)?
    Ok(())
"#
    );
    for optimize in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("main.plenty");
        let executable = dir.path().join("program");
        std::fs::write(&input, &source).unwrap();
        let options = plenty::CompileOptions {
            optimize,
            test_hooks: true,
            ..Default::default()
        };
        plenty::compile_file_to_executable_with_options(&input, &executable, None, &options)
            .unwrap_or_else(|error| panic!("{source}\n{error}"));
        let output = Command::new(executable).output().unwrap();
        assert!(output.status.success(), "{output:?}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        let actual = stdout
            .lines()
            .filter(|line| !line.starts_with("__test_"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            actual,
            "18446744073709551615 18446744073709551615 18446744073709551615 1 9223372036854775807 9223372036854775807 9223372036854775807"
        );
    }
}
