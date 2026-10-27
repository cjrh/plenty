//! `std.actor.tls`: TLS connections for actors over real loopback sockets,
//! with actor clients and an actor server in separate programs. See
//! book/src/design/stdlib/actor-tls.md.
mod support;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::time::Duration;

/// A TLS echo server whose limits each test sets; see the fixture's header.
const SERVER: &str = include_str!("fixtures/actor/tls_server.plenty");

/// Actor clients that send a pattern and check its echo.
const CLIENT: &str = include_str!("fixtures/actor/tls_client.plenty");

/// Replaces each `const NAME: TYPE = VALUE` line named in `settings`, and
/// the certificate directory.
fn configured(source: &str, settings: &[(&str, &str)]) -> String {
    let mut text = source.replace("FIXTURES", &support::tls_fixtures());
    for (name, value) in settings {
        let start = text
            .find(&format!("const {name}: "))
            .unwrap_or_else(|| panic!("no constant {name}"));
        let end = start + text[start..].find('\n').unwrap();
        let kind = text[start..end]
            .split(": ")
            .nth(1)
            .unwrap()
            .split(" = ")
            .next()
            .unwrap()
            .to_string();
        text.replace_range(start..end, &format!("const {name}: {kind} = {value}"));
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

    /// Runs the client fixture against this server and returns its summary.
    fn clients(&self, settings: &[(&str, &str)]) -> String {
        let port = format!("{}u16", self.port);
        let mut all = vec![("port", port.as_str())];
        all.extend_from_slice(settings);
        let (_dir, executable) = support::build_file(&configured(CLIENT, &all));
        let output = Command::new(executable)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(
            output.status.success(),
            "{:?}: {stdout}{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        stdout
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

#[test]
fn actor_clients_and_an_actor_server_exchange_data_and_close_cleanly() {
    let server = Server::start(&[("connections", "2")]);
    let clients = server.clients(&[]);
    assert_eq!(counter(&clients, "matched"), 2, "{clients}");
    assert_clean(&clients, 3);
    let (summary, _) = server.finish();
    for name in ["trapped", "truncated", "rejected", "failed"] {
        assert_eq!(counter(&summary, name), 0, "{summary}");
    }
    // The session, its buffers and the connection, once established.
    let used = counter(&summary, "used");
    assert!(used > 0 && used < 32768, "{summary}");
    assert_clean(&summary, 3);
}

#[test]
fn small_buffers_split_records_and_handshake_flights() {
    // 100 bytes of output holds 68 bytes of plaintext per write; a 64-byte
    // input takes part of a record per read.
    let server = Server::start(&[
        ("connections", "3"),
        ("input_size", "64"),
        ("output_size", "100"),
        ("plain_size", "7"),
    ]);
    let clients = server.clients(&[
        ("clients", "3"),
        ("payload", "20000"),
        ("chunk", "13"),
        ("input_size", "50"),
        ("output_size", "64"),
        ("plain_size", "5"),
    ]);
    assert_eq!(counter(&clients, "matched"), 3, "{clients}");
    let (summary, _) = server.finish();
    assert_eq!(counter(&summary, "failed"), 0, "{summary}");
    assert_clean(&summary, 4);
}

#[test]
fn a_trapping_connection_leaves_the_others_running() {
    let server = Server::start(&[("connections", "2")]);
    let clients = server.clients(&[("mode", "1")]);
    assert_eq!(counter(&clients, "matched"), 1, "{clients}");
    // The trapped actor's socket closes without close_notify.
    assert_eq!(
        counter(&clients, "truncated") + counter(&clients, "cut"),
        1,
        "{clients}"
    );
    let (summary, stderr) = server.finish();
    assert_eq!(counter(&summary, "trapped"), 1, "{summary}");
    assert_eq!(counter(&summary, "failed"), 0, "{summary}");
    assert!(stderr.contains("its heap is discarded"), "{stderr}");
    assert_clean(&summary, 3);
}

#[test]
fn an_untrusted_certificate_fails_both_sides() {
    let server = Server::start(&[("connections", "1")]);
    let clients = server.clients(&[("clients", "1"), ("roots", "\"other-ca.pem\"")]);
    assert_eq!(counter(&clients, "untrusted"), 1, "{clients}");
    assert_clean(&clients, 2);
    let (summary, _) = server.finish();
    assert_eq!(counter(&summary, "rejected"), 1, "{summary}");
    assert_clean(&summary, 2);
}

#[test]
fn a_certificate_for_another_name_fails() {
    let server = Server::start(&[("connections", "1")]);
    let clients = server.clients(&[("clients", "1"), ("server_name", "\"example.com\"")]);
    assert_eq!(counter(&clients, "wrong_host"), 1, "{clients}");
    let (summary, _) = server.finish();
    assert_eq!(counter(&summary, "rejected"), 1, "{summary}");
    assert_clean(&summary, 2);
}

#[test]
fn a_peer_that_closes_without_close_notify_fails_its_connection() {
    let server = Server::start(&[("connections", "2")]);
    let clients = server.clients(&[("mode", "2")]);
    assert_eq!(counter(&clients, "matched"), 2, "{clients}");
    let (summary, _) = server.finish();
    // Unread input turns the close into a reset; otherwise it is Truncated.
    assert_eq!(
        counter(&summary, "truncated") + counter(&summary, "failed"),
        2,
        "{summary}"
    );
    assert_clean(&summary, 3);
}

#[test]
fn plaintext_and_abandoned_handshakes_do_not_affect_a_real_client() {
    let server = Server::start(&[("connections", "3")]);
    let mut plain = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
    plain
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    plain.write_all(b"GET / HTTP/1.1\r\n\r\n").unwrap();
    // The server answers with an alert or nothing, then closes.
    let mut rest = Vec::new();
    drop(plain.read_to_end(&mut rest));
    drop(TcpStream::connect(("127.0.0.1", server.port)).unwrap());
    let clients = server.clients(&[("clients", "1")]);
    assert_eq!(counter(&clients, "matched"), 1, "{clients}");
    let (summary, _) = server.finish();
    assert_eq!(
        counter(&summary, "truncated")
            + counter(&summary, "failed")
            + counter(&summary, "rejected"),
        2,
        "{summary}"
    );
    assert_eq!(counter(&summary, "trapped"), 0, "{summary}");
    assert_clean(&summary, 4);
}

#[test]
fn rustls_exhausting_a_connection_heap_traps_only_that_actor() {
    // A full-size record makes rustls grow its record buffer to 32 KiB,
    // beyond what remains after the 13 KiB an established connection holds.
    let server = Server::start(&[("connections", "2"), ("heap", "24576")]);
    let clients = server.clients(&[("chunk", "16384"), ("output_size", "65536")]);
    assert_eq!(
        counter(&clients, "truncated") + counter(&clients, "cut"),
        2,
        "{clients}"
    );
    let (summary, stderr) = server.finish();
    assert_eq!(counter(&summary, "trapped"), 2, "{summary}");
    assert!(stderr.contains("memory allocation"), "{stderr}");
    assert_clean(&summary, 3);
}
