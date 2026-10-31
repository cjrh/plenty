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
        let mut client = self.connect();
        write!(
            client.get_mut(),
            "GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let reply = response(&mut client, false);
        closed(&mut client);
        reply
    }

    fn finish(mut self) -> String {
        // Also releases the stdin thread if /stop already drained the server.
        self.child.stdin.take().unwrap().write_all(b"x").unwrap();
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
    let server = Server::start(SIMPLE, &[("capacity", "2"), ("timeout_ms", "500u32")]);
    let mut first = server.connect();
    first
        .get_mut()
        .write_all(b"GET / HTTP/1.1\r\nHost:")
        .unwrap();
    let mut second = server.connect();
    second
        .get_mut()
        .write_all(b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\n\r\nx")
        .unwrap();
    // Let admission consume both socket readiness events before opening #3.
    std::thread::sleep(Duration::from_millis(50));
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
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(server.ask("/").body, b"home");
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
    assert_eq!(output, "True\nTrue\nTrue\nTrue\n");
    let summary = server.finish();
    assert_eq!(counter(&summary, "requests"), 4);
    assert_eq!(counter(&summary, "trapped"), 0);
}
