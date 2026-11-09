//! Real socket qualification for the independent actor HTTP server. Each
//! process binds port zero; all socket and process waits have finite bounds.
mod support;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

const COMMON: &str = include_str!("fixtures/actor_http/common.plenty");
const SIMPLE: &str = include_str!("fixtures/actor_http/simple.plenty");
const EVENTS: &str = include_str!("fixtures/actor_http/events.plenty");
const WAIT: Duration = Duration::from_secs(8);

struct Server {
    child: Child,
    lines: Receiver<String>,
    port: u16,
    stderr: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

impl Server {
    fn start(handler: &str, settings: &[(&str, &str)]) -> Self {
        let mut source =
            format!("{COMMON}\n{handler}").replace("FIXTURES", &support::tls_fixtures());
        for (name, value) in settings {
            let start = source.find(&format!("const {name}: ")).unwrap();
            let end = start + source[start..].find('\n').unwrap();
            let equal = start + source[start..end].find(" = ").unwrap();
            source.replace_range(equal + 3..end, value);
        }
        let (dir, executable) = support::build_file(&source);
        let stderr = dir.path().join("stderr");
        let mut child = Command::new(executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(std::fs::File::create(&stderr).unwrap())
            .spawn()
            .unwrap();
        let output = child.stdout.take().unwrap();
        let (sender, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                if sender.send(line.unwrap()).is_err() {
                    break;
                }
            }
        });
        // Install the kill-on-drop guard before any fallible startup wait.
        let mut server = Self {
            child,
            lines,
            port: 0,
            stderr,
            _dir: dir,
        };
        server.port = server
            .lines
            .recv_timeout(WAIT)
            .unwrap_or_else(|error| panic!("server startup: {error}: {}", server.errors()))
            .parse()
            .unwrap();
        server
    }

    fn errors(&self) -> String {
        std::fs::read_to_string(&self.stderr).unwrap()
    }

    fn connect(&self) -> BufReader<TcpStream> {
        let stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        stream.set_read_timeout(Some(WAIT)).unwrap();
        stream.set_write_timeout(Some(WAIT)).unwrap();
        stream.set_nodelay(true).unwrap();
        BufReader::new(stream)
    }

    fn ask(&self, path: &str) -> Reply {
        self.try_ask(path)
            .unwrap_or_else(|| panic!("connection refused: {}", self.errors()))
    }

    /// `None` when the server refuses the connection by closing it
    /// before any response.
    fn try_ask(&self, path: &str) -> Option<Reply> {
        let mut client = self.connect();
        // A refused connection may be reset before the request is written.
        write!(
            client.get_mut(),
            "GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"
        )
        .ok()?;
        match client.fill_buf() {
            Ok([]) => return None,
            Ok(_) => (),
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => return None,
            Err(error) => panic!("reading response: {error}: {}", self.errors()),
        }
        let reply = response(&mut client, false);
        closed(&mut client);
        Some(reply)
    }

    fn finish(mut self) -> String {
        // Also releases the stdin thread if /stop already drained the server.
        let _ = self.child.stdin.take().unwrap().write_all(b"x");
        let deadline = Instant::now() + WAIT;
        let status = loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                break status;
            }
            assert!(
                Instant::now() < deadline,
                "server did not drain: {}",
                self.errors()
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        let summary = self.lines.iter().collect::<Vec<_>>().join("\n");
        assert!(status.success(), "{status}: {summary}\n{}", self.errors());
        assert_eq!(counter(&summary, "heaps"), 0, "{summary}");
        assert_eq!(counter(&summary, "heap_bytes"), 0, "{summary}");
        summary
    }

    #[cfg(target_os = "linux")]
    fn fds(&self) -> usize {
        std::fs::read_dir(format!("/proc/{}/fd", self.child.id()))
            .unwrap()
            .count()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug)]
struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Reply {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

fn response(client: &mut BufReader<TcpStream>, head: bool) -> Reply {
    let mut line = String::new();
    client.read_line(&mut line).unwrap();
    let status = line
        .split_whitespace()
        .nth(1)
        .unwrap_or_else(|| panic!("missing HTTP status: {line:?}"))
        .parse()
        .unwrap();
    let mut reply = Reply {
        status,
        headers: vec![],
        body: vec![],
    };
    loop {
        line.clear();
        assert_ne!(client.read_line(&mut line).unwrap(), 0, "truncated head");
        if line == "\r\n" {
            break;
        }
        let (name, value) = line.trim_end().split_once(':').unwrap();
        reply
            .headers
            .push((name.to_string(), value.trim().to_string()));
    }
    if head || status < 200 || status == 204 || status == 304 {
        return reply;
    }
    if reply.header("transfer-encoding") == Some("chunked") {
        loop {
            line.clear();
            client.read_line(&mut line).unwrap();
            let count = usize::from_str_radix(line.trim(), 16).unwrap();
            if count == 0 {
                line.clear();
                client.read_line(&mut line).unwrap();
                assert_eq!(line, "\r\n");
                break;
            }
            assert!(reply.body.len() + count <= 16 * 1024 * 1024);
            let at = reply.body.len();
            reply.body.resize(at + count, 0);
            client.read_exact(&mut reply.body[at..]).unwrap();
            let mut crlf = [0; 2];
            client.read_exact(&mut crlf).unwrap();
            assert_eq!(&crlf, b"\r\n");
        }
    } else {
        let count: usize = reply.header("content-length").unwrap().parse().unwrap();
        assert!(count <= 16 * 1024 * 1024);
        reply.body.resize(count, 0);
        client.read_exact(&mut reply.body).unwrap();
    }
    reply
}

fn closed(client: &mut BufReader<TcpStream>) {
    let mut byte = [0];
    match client.read(&mut byte) {
        Ok(0) => (),
        Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => (),
        other => panic!("expected closed connection, got {other:?}"),
    }
}

fn counter(summary: &str, name: &str) -> i64 {
    summary
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{name} ")))
        .unwrap_or_else(|| panic!("missing {name}: {summary}"))
        .parse()
        .unwrap()
}

#[test]
fn actor_http_pipeline_head_chunked_trailers_and_continue() {
    let server = Server::start(SIMPLE, &[]);
    let mut client = server.connect();
    client
        .get_mut()
        .write_all(b"HEAD / HTTP/1.1\r\nHost: x\r\n\r\nGET /count HTTP/1.1\r\nHost: x\r\n\r\n")
        .unwrap();
    let head = response(&mut client, true);
    assert_eq!(head.status, 200);
    assert_eq!(head.header("content-length"), Some("4"));
    assert_eq!(response(&mut client, false).body, b"2");
    client
        .get_mut()
        .write_all(
            b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\nExpect: 100-continue\r\n\r\n",
        )
        .unwrap();
    assert_eq!(response(&mut client, false).status, 100);
    client.get_mut().write_all(b"hello").unwrap();
    assert_eq!(response(&mut client, false).body, b"hello");
    // One-byte writes cross chunk-size, extension, data and trailer boundaries.
    let chunked = b"POST /trailers HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n3;x=y\r\nabc\r\n0\r\nX-Trace: yes\r\n\r\n";
    for byte in chunked {
        client.get_mut().write_all(&[*byte]).unwrap();
    }
    assert_eq!(response(&mut client, false).body, b"X-Trace:yes;");
    drop(client);
    assert_eq!(server.ask("/stop").body, b"stopping");
    let summary = server.finish();
    assert_eq!(counter(&summary, "requests"), 5);
    assert_eq!(counter(&summary, "trapped"), 0);
}

#[test]
fn actor_http_malformed_and_oversized_requests_are_refused_without_traps() {
    let server = Server::start(SIMPLE, &[]);
    for (request, expected) in [
        ("GET / HTTP/1.1\r\n\r\n", 400),
        (
            "POST / HTTP/1.1\r\nHost: x\r\nContent-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\n",
            400,
        ),
        (
            "POST / HTTP/1.1\r\nHost: x\r\nContent-Length: 65537\r\n\r\n",
            413,
        ),
        (
            "POST / HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\nZ\r\n",
            400,
        ),
    ] {
        let mut client = server.connect();
        client.get_mut().write_all(request.as_bytes()).unwrap();
        client.get_mut().shutdown(Shutdown::Write).unwrap();
        assert_eq!(response(&mut client, false).status, expected, "{request:?}");
        closed(&mut client);
    }
    assert_eq!(server.ask("/").body, b"home");
    let summary = server.finish();
    assert_eq!(counter(&summary, "trapped"), 0);
    assert_eq!(counter(&summary, "protocol_errors"), 4);
}

#[test]
fn actor_http_repeated_traps_oom_and_handler_errors_preserve_admission() {
    let server = Server::start(SIMPLE, &[]);
    assert_eq!(server.ask("/").status, 200);
    #[cfg(target_os = "linux")]
    let before = server.fds();
    for _ in 0..12 {
        for path in ["/trap", "/oom"] {
            let mut client = server.connect();
            write!(client.get_mut(), "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
            closed(&mut client);
        }
        assert_eq!(server.ask("/error").status, 500);
        assert_eq!(server.ask("/").body, b"home");
    }
    #[cfg(target_os = "linux")]
    {
        // Down notification may be queued after the peer observes EOF.
        let deadline = Instant::now() + Duration::from_secs(2);
        while server.fds() > before && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            server.fds() <= before,
            "socket descriptors leaked across traps"
        );
    }
    let summary = server.finish();
    assert_eq!(counter(&summary, "trapped"), 24, "{summary}");
}

#[test]
fn actor_http_factory_errors_are_contained_and_cancel_drains() {
    let server = Server::start(SIMPLE, &[("factory_fails", "True")]);
    for _ in 0..12 {
        let mut client = server.connect();
        client
            .get_mut()
            .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
            .unwrap();
        closed(&mut client);
    }
    let summary = server.finish();
    assert_eq!(counter(&summary, "trapped"), 0);
    assert!(counter(&summary, "failed") >= 12, "{summary}");
}

#[test]
fn actor_http_capacity_and_slow_head_body_expiry_allow_recovery() {
    // The 2 s timeouts plus the 2 s close linger after each 408 bound how
    // long both slots stay held: #3 must be refused within about 4 s of
    // `first` being accepted.
    let server = Server::start(SIMPLE, &[("capacity", "2"), ("timeout_ms", "2000u32")]);
    let mut first = server.connect();
    first
        .get_mut()
        .write_all(b"GET / HTTP/1.1\r\nHost:")
        .unwrap();
    // Admission accepts in listen-queue order, so the response to the request
    // pipelined ahead of `second`'s slow body proves both connections hold a
    // slot before #3 opens. Its slow bytes arrive in the same write, so no
    // idle gap precedes them.
    let mut second = server.connect();
    second
        .get_mut()
        .write_all(
            b"GET / HTTP/1.1\r\nHost: x\r\n\r\n\
              POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\n\r\nx",
        )
        .unwrap();
    assert_eq!(response(&mut second, false).body, b"home");
    let mut refused = server.connect();
    refused
        .get_mut()
        .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
        .unwrap();
    closed(&mut refused);
    assert_eq!(response(&mut first, false).status, 408);
    assert_eq!(response(&mut second, false).status, 408);
    drop(first);
    drop(second);
    // Slots are freed when the acceptor handles each child's Down mail, which
    // a client cannot observe; retry admission until a connection is served.
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(reply) = server.try_ask("/") {
            assert_eq!(reply.body, b"home");
            break;
        }
        assert!(Instant::now() < deadline, "no slot freed after expiry");
        std::thread::sleep(Duration::from_millis(10));
    }
    let summary = server.finish();
    assert!(counter(&summary, "rejected") >= 1, "{summary}");
    assert_eq!(counter(&summary, "trapped"), 0);
}

#[test]
fn actor_http_many_keepalive_requests_reuse_bounded_connection_state() {
    let server = Server::start(SIMPLE, &[]);
    let mut clients: Vec<_> = (0..8).map(|_| server.connect()).collect();
    for round in 0..20 {
        let size = [0, 1, 255, 1024, 3000][round % 5];
        let body = vec![b'a' + u8::try_from(round % 26).unwrap(); size];
        for client in &mut clients {
            write!(
                client.get_mut(),
                "POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: {size}\r\n\r\n"
            )
            .unwrap();
            client.get_mut().write_all(&body).unwrap();
        }
        for client in &mut clients {
            assert_eq!(response(client, false).body, body);
        }
    }
    drop(clients);
    let summary = server.finish();
    assert_eq!(counter(&summary, "requests"), 160);
    assert_eq!(counter(&summary, "trapped"), 0);
}

#[test]
fn actor_http_streaming_retries_short_writes_and_finishes_during_drain() {
    let server = Server::start(EVENTS, &[]);
    assert_eq!(server.ask("/large").body, vec![b'x'; 262144]);
    let mut delayed = server.connect();
    delayed
        .get_mut()
        .write_all(b"GET /delay HTTP/1.1\r\nHost: x\r\n\r\n")
        .unwrap();
    // Read the response head and first chunk before triggering shutdown.
    let mut line = String::new();
    loop {
        line.clear();
        delayed.read_line(&mut line).unwrap();
        if line == "\r\n" {
            break;
        }
    }
    line.clear();
    delayed.read_line(&mut line).unwrap();
    assert_eq!(line, "5\r\n");
    let mut first = [0; 7];
    delayed.read_exact(&mut first).unwrap();
    assert_eq!(&first, b"first\r\n");
    assert_eq!(server.ask("/stop").status, 200);
    let mut tail = Vec::new();
    delayed.read_to_end(&mut tail).unwrap();
    assert_eq!(tail, b"4\r\nlast\r\n0\r\n\r\n");
    let summary = server.finish();
    assert_eq!(counter(&summary, "requests"), 3);
    assert_eq!(counter(&summary, "trapped"), 0);
}

#[test]
fn actor_http_tls_handshake_timeouts_and_close_notify() {
    let server = Server::start(SIMPLE, &[("secure", "True"), ("timeout_ms", "500u32")]);
    let mut silent = server.connect();
    let mut plaintext = server.connect();
    plaintext
        .get_mut()
        .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
        .unwrap();
    closed(&mut silent);
    // A TLS alert may precede closure after receiving plaintext.
    let mut discarded = Vec::new();
    match plaintext.read_to_end(&mut discarded) {
        Ok(_) => (),
        Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => (),
        other => panic!("plaintext TLS probe did not terminate: {other:?}"),
    }
    #[cfg(target_os = "linux")]
    let before = server.fds();
    let source = include_str!("fixtures/actor_http/tls_client.plenty")
        .replace("PORT", &server.port.to_string())
        .replace("FIXTURES", &support::tls_fixtures());
    let (dir, executable) = support::build_file(&source);
    let output_path = dir.path().join("client-output");
    let output_file = std::fs::File::create(&output_path).unwrap();
    let mut client = Command::new(executable)
        .stdout(output_file.try_clone().unwrap())
        .stderr(output_file)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + WAIT;
    let status = loop {
        if let Some(status) = client.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = client.kill();
            let _ = client.wait();
            panic!(
                "TLS client timed out: {}",
                std::fs::read_to_string(&output_path).unwrap()
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let output = std::fs::read_to_string(output_path).unwrap();
    assert!(status.success(), "{status}: {output}");
    assert_eq!(output, "True\n".repeat(12));
    #[cfg(target_os = "linux")]
    {
        let deadline = Instant::now() + Duration::from_secs(2);
        while server.fds() > before && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            server.fds() <= before,
            "TLS descriptors leaked across traps"
        );
    }
    let summary = server.finish();
    assert_eq!(counter(&summary, "requests"), 4);
    assert_eq!(counter(&summary, "trapped"), 8);
}

#[test]
fn actor_http_stalled_reader_expires_while_other_clients_progress() {
    let server = Server::start(
        EVENTS,
        &[
            ("timeout_ms", "200u32"),
            ("response_timeout_ms", "0u32"),
            ("observe_blocked", "True"),
        ],
    );
    let mut blocked = server.connect();
    blocked
        .get_mut()
        .write_all(b"GET /blocked HTTP/1.1\r\nHost: x\r\n\r\n")
        .unwrap();
    assert_eq!(
        server.lines.recv_timeout(WAIT).unwrap_or_else(|error| {
            panic!(
                "blocked response did not start: {error}: {}",
                server.errors()
            )
        }),
        "blocked started"
    );
    assert_eq!(server.ask("/health").body, b"healthy");
    // Keep all response bytes unread until this connection actor is dropped.
    // A fixed sleep races server scheduling: starting to read too soon frees
    // its socket window and lets the entire response complete (issue #651).
    // Disable the overall response timeout so only the write-stall deadline
    // can retire the still-producing handler. WAIT only bounds this test.
    assert_eq!(
        server.lines.recv_timeout(WAIT).unwrap_or_else(|error| {
            panic!(
                "write deadline did not retire stalled actor: {error}: {}",
                server.errors()
            )
        }),
        "blocked stopped"
    );
    let mut partial = Vec::new();
    match blocked.read_to_end(&mut partial) {
        Ok(_) => (),
        Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => (),
        other => panic!("stalled response did not terminate: {other:?}"),
    }
    assert!(partial.starts_with(b"HTTP/1.1 200"));
    assert!(
        partial.len() < 16777216,
        "write deadline did not cut stalled response"
    );
    drop(blocked);
    let summary = server.finish();
    assert_eq!(counter(&summary, "requests"), 1, "{summary}");
    assert_eq!(counter(&summary, "failed"), 0, "{summary}");
    assert_eq!(counter(&summary, "trapped"), 0, "{summary}");
}

#[test]
fn actor_http_handlers_can_drive_nonblocking_outbound_connections() {
    let upstream = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = upstream.local_addr().unwrap().port();
    upstream.set_nonblocking(true).unwrap();
    let upstream_task = std::thread::spawn(move || {
        let deadline = Instant::now() + WAIT;
        let mut received = 0;
        while received < 2 {
            match upstream.accept() {
                Ok((mut stream, _)) => {
                    stream.set_read_timeout(Some(WAIT)).unwrap();
                    stream.set_write_timeout(Some(WAIT)).unwrap();
                    let mut ping = [0; 4];
                    stream.read_exact(&mut ping).unwrap();
                    assert_eq!(&ping, b"ping");
                    std::thread::sleep(Duration::from_millis(25));
                    stream.write_all(b"upstream").unwrap();
                    received += 1;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "outbound dial did not arrive");
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("upstream accept: {error}"),
            }
        }
    });
    let handler = include_str!("fixtures/actor_http/outbound.plenty");
    let server = Server::start(handler, &[("upstream_port", &format!("{port}u16"))]);
    // Both admission and readiness continue while the first upstream delays.
    let mut first = server.connect();
    let mut second = server.connect();
    for client in [&mut first, &mut second] {
        client
            .get_mut()
            .write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
            .unwrap();
    }
    assert_eq!(response(&mut first, false).body, b"upstream");
    assert_eq!(response(&mut second, false).body, b"upstream");
    upstream_task.join().unwrap();
    drop((first, second));
    let summary = server.finish();
    assert_eq!(counter(&summary, "requests"), 2);
    assert_eq!(counter(&summary, "trapped"), 0);
}

#[test]
fn actor_http_whole_response_limits_are_atomic_and_bodyless_statuses_have_no_length() {
    let server = Server::start(SIMPLE, &[]);
    for (path, status) in [("/empty204", 204), ("/empty304", 304)] {
        let reply = server.ask(path);
        assert_eq!(reply.status, status);
        assert_eq!(reply.header("content-length"), None);
        assert_eq!(reply.header("transfer-encoding"), None);
    }
    assert_eq!(server.ask("/invalid204").status, 500);
    let mut explicit = server.connect();
    explicit
        .get_mut()
        .write_all(b"GET /explicit-close HTTP/1.1\r\nHost: x\r\n\r\n")
        .unwrap();
    assert_eq!(response(&mut explicit, false).body, b"bye");
    closed(&mut explicit);
    let mut client = server.connect();
    client
        .get_mut()
        .write_all(b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 4000\r\n\r\n")
        .unwrap();
    client.get_mut().write_all(&vec![b'x'; 4000]).unwrap();
    // Body alone fits pending_output; encoded head plus body does not.
    // Failure must still permit a complete500, without a partial200 prefix.
    assert_eq!(response(&mut client, false).status, 500);
    closed(&mut client);
    assert_eq!(server.ask("/").status, 200);
    assert_eq!(counter(&server.finish(), "trapped"), 0);
}

#[test]
fn actor_http_custom_head_limits_http10_expect_and_blank_eof() {
    let server = Server::start(SIMPLE, &[("max_line", "16384"), ("max_head", "65536")]);
    let mut client = server.connect();
    write!(
        client.get_mut(),
        "GET / HTTP/1.1\r\nHost: x\r\nX-Large: {}\r\nConnection: close\r\n\r\n",
        "v".repeat(9000)
    )
    .unwrap();
    assert_eq!(response(&mut client, false).body, b"home");
    closed(&mut client);
    let mut old = server.connect();
    old.get_mut()
        .write_all(b"POST /echo HTTP/1.0\r\nContent-Length: 2\r\nExpect: 100-continue\r\n\r\nhi")
        .unwrap();
    let reply = response(&mut old, false);
    assert_eq!(reply.status, 200);
    assert_eq!(reply.body, b"hi");
    closed(&mut old);
    let mut blank = server.connect();
    blank.get_mut().write_all(b"\r\n\r\n").unwrap();
    blank.get_mut().shutdown(Shutdown::Write).unwrap();
    closed(&mut blank);
    assert_eq!(counter(&server.finish(), "trapped"), 0);
}

#[test]
fn actor_http_force_drain_reclaims_actor_with_perpetually_full_mailbox() {
    // Only the 2 s drain deadline can end the busy response: its response
    // timeout is longer than `finish`'s own wait.
    let server = Server::start(EVENTS, &[("response_timeout_ms", "60000u32")]);
    let mut client = server.connect();
    client
        .get_mut()
        .write_all(b"GET /busy HTTP/1.1\r\nHost: x\r\n\r\n")
        .unwrap();
    let mut line = String::new();
    loop {
        line.clear();
        client.read_line(&mut line).unwrap();
        if line == "\r\n" {
            break;
        }
    }
    let start = Instant::now();
    let summary = server.finish();
    assert!(start.elapsed() >= Duration::from_secs(1));
    assert_eq!(counter(&summary, "trapped"), 0);
    assert_eq!(counter(&summary, "requests"), 0);
}

#[test]
fn actor_http_sigterm_finishes_an_inflight_response_before_exit() {
    let server = Server::start(EVENTS, &[("signal_stop", "True")]);
    let mut client = server.connect();
    client
        .get_mut()
        .write_all(b"GET /delay HTTP/1.1\r\nHost: x\r\n\r\n")
        .unwrap();
    let mut line = String::new();
    loop {
        line.clear();
        client.read_line(&mut line).unwrap();
        if line == "\r\n" {
            break;
        }
    }
    line.clear();
    client.read_line(&mut line).unwrap();
    assert_eq!(line, "5\r\n");
    let mut first = [0; 7];
    client.read_exact(&mut first).unwrap();
    assert_eq!(&first, b"first\r\n");
    assert!(
        Command::new("kill")
            .args(["-TERM", &server.child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let mut tail = Vec::new();
    client.read_to_end(&mut tail).unwrap();
    assert_eq!(tail, b"4\r\nlast\r\n0\r\n\r\n");
    let summary = server.finish();
    assert_eq!(counter(&summary, "requests"), 1);
    assert_eq!(counter(&summary, "trapped"), 0);
}

fn request_https_shutdown(server: &Server) {
    let code = r#"import socket, ssl, sys
context = ssl.create_default_context(cafile=sys.argv[2])
stream = context.wrap_socket(socket.create_connection(('127.0.0.1', int(sys.argv[1])), timeout=5), server_hostname='localhost')
stream.sendall(b'GET /stop HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n')
received = b''
while b'\r\n\r\n' not in received:
    received += stream.recv(1)
assert received.startswith(b'HTTP/1.1 200')
body = b''
while len(body) < 8:
    piece = stream.recv(8 - len(body))
    assert piece
    body += piece
assert body == b'stopping'
# Deliberately omit unwrap(): close the TCP stream without TLS close_notify.
stream.close()
"#;
    let output = Command::new("python3")
        .args([
            "-c",
            code,
            &server.port.to_string(),
            &format!("{}/ca.pem", support::tls_fixtures()),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn actor_https_shutdown_request_survives_peer_omitting_close_notify() {
    let server = Server::start(SIMPLE, &[("secure", "True")]);
    request_https_shutdown(&server);
    // A report proves serve returned before stdin cancellation. The fixture
    // deliberately joins its stdin reader only after printing these lines.
    assert_eq!(
        server
            .lines
            .recv_timeout(Duration::from_secs(4))
            .expect("TLS peer close lost shutdown request"),
        "requests 1"
    );
    let summary = server.finish();
    assert_eq!(counter(&summary, "trapped"), 0);
}

#[test]
fn actor_http_connection_churn_reclaims_admission_slots_while_mail_keeps_arriving() {
    let server = Server::start(SIMPLE, &[("capacity", "64")]);
    let port = server.port;
    std::thread::scope(|scope| {
        let mut clients = Vec::new();
        for _ in 0..16 {
            clients.push(scope.spawn(move || {
                for _ in 0..320 {
                    let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
                    stream.set_read_timeout(Some(WAIT)).unwrap();
                    stream.set_write_timeout(Some(WAIT)).unwrap();
                    let mut client = BufReader::new(stream);
                    client
                        .get_mut()
                        .write_all(b"GET / HTTP/1.0\r\n\r\n")
                        .unwrap();
                    assert_eq!(response(&mut client, false).body, b"home");
                    closed(&mut client);
                }
            }));
        }
        for client in clients {
            client.join().unwrap();
        }
    });
    assert_eq!(server.ask("/").body, b"home");
    let summary = server.finish();
    assert_eq!(counter(&summary, "requests"), 5121);
    assert_eq!(counter(&summary, "rejected"), 0);
    assert_eq!(counter(&summary, "trapped"), 0);
}

#[test]
fn actor_https_many_new_connections_preserve_admission_and_shutdown_control() {
    let server = Server::start(
        SIMPLE,
        &[
            ("secure", "True"),
            ("capacity", "256"),
            ("timeout_ms", "5000u32"),
        ],
    );
    // Separate processes bound the whole workload as well as each socket wait.
    // The client uses fresh connections and 16 workers, including on release
    // images whose system curl predates parallel mode (issue #670).
    let output = Command::new("python3")
        .args([
            "-c",
            "import subprocess,sys; raise SystemExit(subprocess.run(sys.argv[1:], timeout=40).returncode)",
            "python3", "-c", include_str!("fixtures/actor/http_churn.py"),
            &server.port.to_string(),
            &format!("{}/ca.pem", support::tls_fixtures()),
        ])
        .output().unwrap();
    let statuses = String::from_utf8(output.stdout).unwrap();
    let failures: Vec<_> = statuses
        .lines()
        .filter(|line| !line.starts_with("200 "))
        .take(16)
        .collect();
    if !output.status.success() || statuses.lines().count() != 5000 || !failures.is_empty() {
        // Cancel through stdin and collect accounting even when the client fails;
        // dropping the process at the assertion would lose this evidence.
        let errors = server.errors();
        let summary = server.finish();
        panic!(
            "HTTPS churn: client {}; {} transfers; failures: {failures:?}\n{}\nserver:\n{summary}\n{errors}",
            output.status,
            statuses.lines().count(),
            String::from_utf8_lossy(&output.stderr),
        );
    }
    request_https_shutdown(&server);
    assert_eq!(
        server
            .lines
            .recv_timeout(Duration::from_secs(4))
            .expect("shutdown stalled after HTTPS churn"),
        "requests 5001"
    );
    let summary = server.finish();
    assert_eq!(counter(&summary, "rejected"), 0);
    assert_eq!(counter(&summary, "trapped"), 0);
}

#[test]
fn actor_http_mixed_traps_and_stalled_heads_preserve_healthy_admission() {
    let server = Server::start(SIMPLE, &[("capacity", "64"), ("timeout_ms", "30000u32")]);
    let mut stalled: Vec<_> = (0..32).map(|_| server.connect()).collect();
    for client in &mut stalled {
        client
            .get_mut()
            .write_all(b"GET / HTTP/1.1\r\nHost:")
            .unwrap();
    }
    let port = server.port;
    std::thread::scope(|scope| {
        let mut clients = Vec::new();
        for _ in 0..8 {
            clients.push(scope.spawn(move || {
                for turn in 0..64 {
                    if turn % 8 == 0 {
                        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
                        stream.set_read_timeout(Some(WAIT)).unwrap();
                        stream.set_write_timeout(Some(WAIT)).unwrap();
                        let mut trapped = BufReader::new(stream);
                        trapped
                            .get_mut()
                            .write_all(b"GET /trap HTTP/1.1\r\nHost: x\r\n\r\n")
                            .unwrap();
                        closed(&mut trapped);
                    }
                    let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
                    stream.set_read_timeout(Some(WAIT)).unwrap();
                    stream.set_write_timeout(Some(WAIT)).unwrap();
                    let mut client = BufReader::new(stream);
                    client
                        .get_mut()
                        .write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
                        .unwrap();
                    assert_eq!(response(&mut client, false).body, b"home");
                    closed(&mut client);
                }
            }));
        }
        for client in clients {
            client.join().unwrap();
        }
    });
    // Keep incomplete heads alive through cancellation; the force deadline
    // must reclaim them while completed healthy responses remain counted.
    let summary = server.finish();
    drop(stalled);
    assert_eq!(counter(&summary, "requests"), 512);
    assert_eq!(counter(&summary, "rejected"), 0);
    assert_eq!(counter(&summary, "trapped"), 64);
}

#[test]
fn actor_http_completion_pressure_preserves_application_messages_and_timers() {
    let server = Server::start(
        include_str!("fixtures/actor_http/notices.plenty"),
        &[("capacity", "128"), ("timeout_ms", "5000u32")],
    );
    let mut clients: Vec<_> = (0..64).map(|_| server.connect()).collect();
    for client in &mut clients {
        client
            .get_mut()
            .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
            .unwrap();
    }
    for client in &mut clients {
        assert_eq!(response(client, false).body, b"scheduled");
    }
    // No wait is needed before /count. Each self-message was queued before
    // the next request's Ready, and the stage alternates due notices with
    // mailbox mail, so the 0 s timer is delivered before that Ready too: at
    // most one private retry timer can be due ahead of it. See
    // `deliver` in stdlib/std/actor.plenty and book/src/design/stdlib/actor.md.
    for client in &mut clients {
        client
            .get_mut()
            .write_all(b"GET /count HTTP/1.1\r\nHost: x\r\n\r\n")
            .unwrap();
    }
    for client in &mut clients {
        assert_eq!(response(client, false).body, b"2");
    }
    drop(clients);
    let summary = server.finish();
    assert_eq!(counter(&summary, "requests"), 128);
    assert_eq!(counter(&summary, "trapped"), 0);
}

#[test]
fn actor_http_completion_pressure_has_bounded_retry_work() {
    const CLIENTS: i64 = 64;
    const PIPELINED: i64 = 8;
    let server = Server::start(SIMPLE, &[("capacity", "128"), ("timeout_ms", "5000u32")]);
    let mut clients: Vec<_> = (0..CLIENTS).map(|_| server.connect()).collect();
    // Confirm admission before submitting the simultaneous pipelined burst.
    for client in &mut clients {
        client
            .get_mut()
            .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
            .unwrap();
        assert_eq!(response(client, false).body, b"home");
    }
    let pipeline =
        b"GET /count HTTP/1.1\r\nHost: x\r\n\r\n".repeat(usize::try_from(PIPELINED).unwrap());
    for client in &mut clients {
        client.get_mut().write_all(&pipeline).unwrap();
    }
    for client in &mut clients {
        for request in 2..=PIPELINED + 1 {
            assert_eq!(response(client, false).body, request.to_string().as_bytes());
        }
    }
    drop(clients);
    let summary = server.finish();
    let requests = CLIENTS * (PIPELINED + 1);
    assert_eq!(counter(&summary, "requests"), requests, "{summary}");
    assert_eq!(counter(&summary, "trapped"), 0, "{summary}");
    assert_eq!(counter(&summary, "failed"), 0, "{summary}");
    assert_eq!(counter(&summary, "lost"), 0, "{summary}");
    // A work bound, not a wall-clock performance assertion: the old repeated
    // self-continuations made dozens of attempts per completed response.
    assert!(counter(&summary, "full") < requests * 32, "{summary}");
    assert!(counter(&summary, "delivered") < requests * 32, "{summary}");
}

#[test]
fn actor_http_peer_eof_during_completion_closes_without_waiting_for_idle_timeout() {
    let server = Server::start(SIMPLE, &[("timeout_ms", "5000u32")]);
    let mut client = server.connect();
    // Half-close before reading: EOF can arrive while a completed response
    // still awaits admission's acknowledgement. No new readiness will follow.
    client
        .get_mut()
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client
        .get_mut()
        .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\nGET /count HTTP/1.1\r\nHost: x\r\n\r\n")
        .unwrap();
    client.get_mut().shutdown(Shutdown::Write).unwrap();
    assert_eq!(response(&mut client, false).body, b"home");
    assert_eq!(response(&mut client, false).body, b"2");
    closed(&mut client);
    drop(client);
    let summary = server.finish();
    assert_eq!(counter(&summary, "requests"), 2, "{summary}");
    assert_eq!(counter(&summary, "failed"), 0, "{summary}");
    assert_eq!(counter(&summary, "trapped"), 0, "{summary}");
}

#[test]
fn actor_http_completion_control_survives_exhausted_timer_pool() {
    let server = Server::start(
        include_str!("fixtures/actor_http/control_pressure.plenty"),
        &[("capacity", "128"), ("timeout_ms", "5000u32")],
    );
    let mut clients: Vec<_> = (0..64).map(|_| server.connect()).collect();
    // Every connection must own its transport and phase timers before the
    // application consumes the remaining shared timer records.
    for client in &mut clients {
        client
            .get_mut()
            .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
            .unwrap();
        assert_eq!(response(client, false).body, b"ready");
    }
    for client in &mut clients {
        client
            .get_mut()
            .write_all(b"GET /exhaust HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
            .unwrap();
    }
    for client in &mut clients {
        assert_eq!(response(client, false).body, b"exhausted");
        closed(client);
    }
    drop(clients);
    let summary = server.finish();
    assert_eq!(counter(&summary, "requests"), 128, "{summary}");
    assert_eq!(counter(&summary, "failed"), 0, "{summary}");
    assert_eq!(counter(&summary, "trapped"), 0, "{summary}");
    assert_eq!(counter(&summary, "lost"), 0, "{summary}");
    assert!(counter(&summary, "full") > 0, "{summary}");
}

#[test]
fn actor_http_completed_responses_survive_handler_errors() {
    // Full attempts depend on scheduling; acknowledgement pressure is tested
    // separately with a controlled mailbox. This checks delivery and teardown.
    let server = Server::start(
        include_str!("fixtures/actor_http/control_pressure.plenty"),
        &[("capacity", "128"), ("timeout_ms", "5000u32")],
    );
    let mut clients: Vec<_> = (0..64).map(|_| server.connect()).collect();
    for client in &mut clients {
        client
            .get_mut()
            .write_all(b"GET /fail HTTP/1.1\r\nHost: x\r\n\r\n")
            .unwrap();
    }
    for client in &mut clients {
        assert_eq!(response(client, false).body, b"sent");
        closed(client);
    }
    drop(clients);
    let summary = server.finish();
    assert_eq!(counter(&summary, "requests"), 64, "{summary}");
    assert_eq!(counter(&summary, "failed"), 64, "{summary}");
    assert_eq!(counter(&summary, "trapped"), 0, "{summary}");
    assert_eq!(counter(&summary, "lost"), 0, "{summary}");
}

#[test]
fn actor_http_acknowledgements_retry_a_full_mailbox() {
    // Compile the unmodified module with a same-module probe so private control
    // state can be inspected without exposing test hooks in the public API.
    let source = format!(
        "{}\n{}",
        include_str!("../stdlib/std/actor/http/server.plenty"),
        include_str!("fixtures/actor_http/acknowledgements.plenty"),
    );
    let (_dir, executable) = support::build_file(&source);
    let output = support::output_within(&mut Command::new(executable), b"", WAIT);
    let output = support::assert_success(output);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "pending 1\nacknowledged 16\nretried True\nlost 0\nheaps 0\nheap_bytes 0\n"
    );
}

fn arithmetic_probe(source: &str, optimize: bool) -> String {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&input, source).unwrap();
    let options = plenty::CompileOptions {
        optimize,
        test_hooks: true,
        ..Default::default()
    };
    plenty::compile_file_to_executable_with_options(&input, &executable, None, &options)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let output = support::output_within(&mut Command::new(executable), b"", WAIT);
    let output = support::assert_success(output);
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn actor_http_server_arithmetic_representation_boundaries() {
    let source = format!(
        "{}\n{}",
        include_str!("../stdlib/std/actor/http/server.plenty"),
        include_str!("fixtures/actor_http/arithmetic.plenty"),
    );
    for optimize in [false, true] {
        assert_eq!(
            arithmetic_probe(&source, optimize),
            "1 18446744073709551615 18446744073709551615\n16777215 16777216 16777216 16777216 16777216\n0 448 9223372036854775743 0"
        );
    }
}

#[test]
fn actor_http_lingering_preserves_consumption_and_close_order() {
    let server = include_str!("../stdlib/std/actor/http/server.plenty");
    let helper = server
        .split("def lingering_after(")
        .nth(1)
        .unwrap()
        .split("def response(")
        .next()
        .unwrap();
    // Extract the production phase branch, keeping consumption/error/close
    // ordering intact. Only transport and lifecycle callbacks are modelled.
    let branch = server
        .split("        if self.phase == 5:\n")
        .nth(1)
        .unwrap()
        .split("        if self.phase == 1 ")
        .next()
        .unwrap();
    let branch = branch
        .lines()
        .map(|line| line.strip_prefix("    ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n");
    let source = format!(
        r#"
def lingering_after({helper}
enum ConsumeError:
    Consume

class WireProbe:
    count: i64
    output: i64
    reads: bool
    fail: bool
    consumed: i64
    closed: bool
    def available(self) -> i64:
        self.count
    def reading(self) -> bool:
        self.reads
    def pending(self) -> i64:
        self.output
    def consume(self: &mut WireProbe, count: i64, context: i64) -> Result[(), ConsumeError]:
        if self.fail:
            return Err(ConsumeError.Consume)
        self.consumed = count
        self.count = 0
        Ok(())
    def close(self: &mut WireProbe, context: i64) -> Result[(), ConsumeError]:
        self.closed = True
        Ok(())

class ClientProbe:
    lingering: i64
    stopped: bool
    resumed: bool
    def stop(self: &mut ClientProbe, context: i64) -> Result[(), ConsumeError]:
        self.stopped = True
        Ok(())
    def resume(self: &mut ClientProbe, context: i64) -> Result[(), ConsumeError]:
        self.resumed = True
        Ok(())
    def advance(self: &mut ClientProbe, wire: &mut WireProbe, context: i64) -> Result[(), ConsumeError]:
{branch}

def run(current: i64, count: i64, pending: i64, reading: bool, fail: bool) -> Result[(), IoError | AllocError]:
    mut client = ClientProbe(current, False, False)
    mut wire = WireProbe(count, pending, reading, fail, 0, False)
    print("__test_begin_no_allocations__")?
    result = client.advance(&mut wire, 0)
    print("__test_end_no_allocations__")?
    print(result.is_err(), client.lingering, wire.consumed, wire.closed, client.stopped, client.resumed)?
    Ok(())

def main() -> Result[(), IoError | AllocError]:
    run(16777215, 0, 0, True, False)?
    run(16777215, 1, 0, True, False)?
    run(16777215, i64.MAX, 1, True, False)?
    run(16777216, i64.MAX, 0, True, False)?
    run(0, 1, 0, False, False)?
    run(16777215, i64.MAX, 0, True, True)?
    Ok(())
"#
    );
    for optimize in [false, true] {
        assert_eq!(
            arithmetic_probe(&source, optimize),
            concat!(
                "False 16777215 0 False False False\n",
                "False 16777216 1 True True False\n",
                "False 16777216 9223372036854775807 False False False\n",
                "False 16777216 9223372036854775807 True True False\n",
                "False 1 1 True True False\n",
                "True 16777216 0 False False False"
            )
        );
    }
}
