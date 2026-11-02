//! The worker server's retained wire corpus and deterministic mutations,
//! replayed through real actor connections. Only the scheduling differs.
mod support;

use super::test_std_http_server_qualification::{corpus, mutations, statuses};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

const SERVER: &str = include_str!("fixtures/actor_http/corpus.plenty");
const WAIT: Duration = Duration::from_secs(5);
const HEALTH: &[u8] = b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n";

pub(super) struct Server {
    child: Child,
    lines: Receiver<String>,
    pub(super) port: u16,
    stderr: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

impl Server {
    fn start() -> Self {
        Self::start_with(SERVER)
    }

    pub(super) fn start_with(source: &str) -> Self {
        let (dir, executable) = support::build_file(source);
        let stderr = dir.path().join("stderr");
        let mut child = Command::new(executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(std::fs::File::create(&stderr).unwrap())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if sender.send(line.unwrap()).is_err() {
                    break;
                }
            }
        });
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

    fn replay(&self, input: &[u8], step: usize) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        stream.set_read_timeout(Some(WAIT)).unwrap();
        stream.set_write_timeout(Some(WAIT)).unwrap();
        stream.set_nodelay(true).unwrap();
        let deadline = Instant::now() + WAIT;
        for piece in input.chunks(step) {
            assert!(Instant::now() < deadline, "request write exceeded deadline");
            match stream.write_all(piece) {
                Ok(()) => (),
                Err(error) if closed_error(&error) => break,
                Err(error) => panic!("request write failed: {error}"),
            }
        }
        // Matches Script's EOF without waiting for the server's idle timer.
        if let Err(error) = stream.shutdown(Shutdown::Write) {
            assert!(closed_error(&error), "halfclose failed: {error}");
        }
        let mut output = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(!remaining.is_zero(), "response exceeded deadline");
            stream.set_read_timeout(Some(remaining)).unwrap();
            match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    output.extend_from_slice(&buffer[..count]);
                    assert!(output.len() <= 2 * 1024 * 1024, "unbounded response");
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) if closed_error(&error) => break,
                Err(error) => panic!("response read failed: {error}; server: {}", self.errors()),
            }
        }
        statuses(&String::from_utf8_lossy(&output))
    }

    fn assert_healthy(&self) {
        assert_eq!(
            self.replay(HEALTH, 64),
            "200",
            "listener stopped progressing"
        );
    }

    pub(super) fn finish(mut self) {
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
        // Containment must not hide malformed-input parser traps.
        for name in ["trapped", "failed", "heaps", "heap_bytes"] {
            let expected = format!("{name} 0");
            assert!(
                summary.lines().any(|line| line == expected),
                "{summary}\n{}",
                self.errors()
            );
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn closed_error(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::NotConnected
    )
}

#[test]
fn actor_http_shared_corpus_has_identical_statuses_on_real_sockets() {
    let server = Server::start();
    let cases = corpus();
    assert!(cases.len() >= 50);
    // Socket writes may be coalesced by the kernel. Decoder unit tests also
    // prove exact single-byte feed boundaries independently of transport.
    for step in [1, 64] {
        for (index, (expected, input)) in cases.iter().enumerate() {
            assert_eq!(
                server.replay(input, step),
                *expected,
                "case {index}, write step {step}, request {:?}",
                String::from_utf8_lossy(input)
            );
        }
        server.assert_healthy();
    }
    server.finish();
}

#[test]
fn actor_http_shared_mutations_never_trap_or_disable_admission() {
    let server = Server::start();
    let inputs = mutations();
    assert_eq!(inputs.len(), 600);
    let allowed = [
        "100", "200", "400", "404", "405", "413", "431", "500", "501", "505",
    ];
    for (index, input) in inputs.iter().enumerate() {
        let actual = server.replay(input, 7);
        assert!(
            actual == "-" || actual.split(' ').all(|status| allowed.contains(&status)),
            "mutation {index}: {:?} -> {actual}",
            String::from_utf8_lossy(input)
        );
        if index % 25 == 0 {
            server.assert_healthy();
        }
    }
    server.assert_healthy();
    server.finish();
}
