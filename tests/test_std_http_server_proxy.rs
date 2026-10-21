//! `std.http.server` behind real reverse proxies (issue #103, B66): nginx and
//! Caddy, each in a stock container in front of a server running on the host.
//! Keep-alive, streamed and chunked bodies, `Expect: 100-continue`, forwarded
//! `Host`, `X-Forwarded-For` and `X-Forwarded-Proto`, TLS terminated at the
//! proxy, slow clients and upstreams, refusals with a lingering close, and a
//! drain on `SIGTERM`. See "Reverse proxies" in
//! `book/src/design/stdlib/http-qualification.md`.
//!
//! The tests need Docker and curl and run only with `PLENTY_DOCKER_TESTS=1`;
//! otherwise each prints that it is skipped. With the variable set, a missing
//! Docker or curl fails them. The proxy runs with `--network host` and listens
//! on loopback ports chosen at run time; the server program, linked against
//! the host's C library, runs on the host.

mod support;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const SERVER: &str = include_str!("fixtures/proxy/server.plenty");
const NGINX_CONF: &str = include_str!("fixtures/proxy/nginx.conf");
const CADDYFILE: &str = include_str!("fixtures/proxy/Caddyfile");

/// Exact images; the qualification page records the same versions.
const NGINX_IMAGE: &str = "nginx:1.30.6-alpine3.24";
const CADDY_IMAGE: &str = "caddy:2.11.7-alpine";

/// Where the configuration and certificates are mounted in the container.
const MOUNT: &str = "/etc/plenty-proxy";

const SIGTERM: i32 = 15;

unsafe extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
}

/// Whether to run: `PLENTY_DOCKER_TESTS=1` with Docker and curl usable.
fn enabled(test: &str) -> bool {
    if std::env::var("PLENTY_DOCKER_TESTS").as_deref() != Ok("1") {
        // Written past the test harness's capture, so the skip is visible.
        let _ = writeln!(
            std::io::stderr(),
            "{test}: skipped; set PLENTY_DOCKER_TESTS=1 to run the Docker proxy tests"
        );
        return false;
    }
    let docker = Command::new("docker")
        .args(["info", "--format", "{{.ServerVersion}}"])
        .output();
    match docker {
        Ok(output) if output.status.success() => {}
        Ok(output) => panic!(
            "PLENTY_DOCKER_TESTS=1 but the Docker daemon is unavailable: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
        Err(error) => panic!("PLENTY_DOCKER_TESTS=1 but docker cannot run: {error}"),
    }
    if let Err(error) = Command::new("curl").arg("--version").output() {
        panic!("PLENTY_DOCKER_TESTS=1 but curl cannot run: {error}");
    }
    true
}

/// A loopback port that was free a moment ago.
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// The server program on the host. It is killed if the test ends early.
struct Server {
    child: Child,
    lines: BufReader<ChildStdout>,
    port: u16,
    _dir: tempfile::TempDir,
}

impl Server {
    fn start() -> Server {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("main.plenty");
        let built = dir.path().join("built");
        let executable = dir.path().join("server");
        std::fs::write(&source, SERVER).unwrap();
        plenty::compile_file_to_executable(&source, &built, None)
            .unwrap_or_else(|error| panic!("{error}"));
        support::install_executable(&built, &executable);
        let mut child = Command::new(executable)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        lines.read_line(&mut line).unwrap();
        let port = line
            .trim()
            .strip_prefix("port ")
            .unwrap_or_else(|| panic!("no port line: {line:?}"))
            .parse()
            .unwrap();
        Server {
            child,
            lines,
            port,
            _dir: dir,
        }
    }

    /// Sends `SIGTERM`, then returns the report the server prints as it exits.
    fn terminate(&mut self) -> String {
        // SAFETY: plain integer arguments; the child is unreaped, so its pid is ours.
        assert_eq!(unsafe { kill(self.child.id() as i32, SIGTERM) }, 0);
        let mut rest = String::new();
        self.lines.read_to_string(&mut rest).unwrap();
        let status = self.child.wait().unwrap();
        assert!(status.success(), "{status:?}\n{rest}");
        rest.trim_end().to_owned()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Nginx,
    Caddy,
}

/// A proxy container in front of a server, removed when dropped (also when
/// the test panics). Only this container is ever stopped or removed.
struct Proxy {
    kind: Kind,
    name: String,
    http: u16,
    tls: u16,
    docker: Child,
    dir: tempfile::TempDir,
}

impl Proxy {
    fn start(kind: Kind, upstream: u16) -> Proxy {
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        let (image, template, config, command): (_, _, _, &[&str]) = match kind {
            Kind::Nginx => (
                NGINX_IMAGE,
                NGINX_CONF,
                "nginx.conf",
                &[
                    "nginx",
                    "-c",
                    "/etc/plenty-proxy/nginx.conf",
                    "-g",
                    "daemon off;",
                ],
            ),
            Kind::Caddy => (
                CADDY_IMAGE,
                CADDYFILE,
                "Caddyfile",
                &[
                    "caddy",
                    "run",
                    "--config",
                    "/etc/plenty-proxy/Caddyfile",
                    "--adapter",
                    "caddyfile",
                ],
            ),
        };
        let present = Command::new("docker")
            .args(["image", "inspect", image])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        if !present.success() {
            let pulled = Command::new("docker")
                .args(["pull", image])
                .output()
                .unwrap();
            assert!(
                pulled.status.success(),
                "docker pull {image}: {}",
                String::from_utf8_lossy(&pulled.stderr)
            );
        }
        let (http, tls) = (free_port(), free_port());
        let dir = tempfile::tempdir().unwrap();
        let text = template
            .replace("@UPSTREAM_PORT@", &upstream.to_string())
            .replace("@HTTP_PORT@", &http.to_string())
            .replace("@TLS_PORT@", &tls.to_string());
        std::fs::write(dir.path().join(config), text).unwrap();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tls");
        for file in ["leaf.pem", "leaf.key"] {
            std::fs::copy(fixtures.join(file), dir.path().join(file)).unwrap();
        }
        let name = format!(
            "plenty-proxy-test-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        );
        let log = std::fs::File::create(dir.path().join("log")).unwrap();
        // `z` relabels the mount for SELinux hosts; the files are ours.
        let docker = Command::new("docker")
            .args(["run", "--rm", "--name", &name, "--network", "host"])
            .args(["--label", "plenty-test=proxy", "-v"])
            .arg(format!("{}:{MOUNT}:ro,z", dir.path().display()))
            .arg(image)
            .args(command)
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap();
        let mut proxy = Proxy {
            kind,
            name,
            http,
            tls,
            docker,
            dir,
        };
        proxy.wait_listening();
        proxy
    }

    fn wait_listening(&mut self) {
        let start = Instant::now();
        for port in [self.http, self.tls] {
            while TcpStream::connect(("127.0.0.1", port)).is_err() {
                if let Ok(Some(status)) = self.docker.try_wait() {
                    panic!("proxy exited with {status}:\n{}", self.log());
                }
                assert!(
                    start.elapsed() < Duration::from_secs(60),
                    "proxy not listening on {port}:\n{}",
                    self.log()
                );
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("log")).unwrap_or_default()
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.http)
    }

    fn tls_url(&self, path: &str) -> String {
        format!("https://localhost:{}{path}", self.tls)
    }

    /// curl arguments that reach the TLS port and trust the test CA.
    fn tls_args(&self) -> Vec<String> {
        let ca = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tls/ca.pem");
        vec![
            "--cacert".into(),
            ca.display().to_string(),
            "--resolve".into(),
            format!("localhost:{}:127.0.0.1", self.tls),
        ]
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("proxy {} log:\n{}", self.name, self.log());
        }
        let _ = Command::new("docker")
            .args(["rm", "--force", &self.name])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = self.docker.kill();
        let _ = self.docker.wait();
    }
}

/// curl, which must succeed. Requests come from 127.0.0.2, so the server can
/// tell the client from the proxy, which connects from 127.0.0.1.
fn curl(args: &[&str]) -> Output {
    let output = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--max-time",
            "30",
            "--noproxy",
            "*",
        ])
        .args(["--interface", "127.0.0.2"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "curl {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

/// The status code and the body as text.
fn fetch(args: &[&str]) -> (String, String) {
    let mut all = vec!["-w", "\n%{http_code}"];
    all.extend(args);
    let text = String::from_utf8(curl(&all).stdout).unwrap();
    let (body, code) = text.rsplit_once('\n').unwrap();
    (code.to_owned(), body.to_owned())
}

/// `fetch` for an answer Caddy relays before it has read the whole request
/// body. Caddy then closes the client connection with the rest unread, so
/// curl, still sending, may fail with a reset (exit 55) after receiving
/// the answer. That is between the client and the proxy; the status shows
/// whether the server's answer reached the proxy.
fn relayed(args: &[&str]) -> (String, String) {
    let output = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--max-time",
            "30",
            "--noproxy",
            "*",
        ])
        .args(["--interface", "127.0.0.2", "-w", "\n%{http_code}"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        matches!(output.status.code(), Some(0 | 55)),
        "curl {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let (body, code) = text.rsplit_once('\n').unwrap();
    (code.to_owned(), body.to_owned())
}

fn connect(port: u16) -> TcpStream {
    let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    stream
}

/// Reads until `received` contains `marker`.
fn read_until(stream: &mut TcpStream, received: &mut Vec<u8>, marker: &str) {
    let mut buffer = [0u8; 4096];
    while !String::from_utf8_lossy(received).contains(marker) {
        let count = stream.read(&mut buffer).unwrap();
        assert!(
            count > 0,
            "ended before {marker:?}: {}",
            String::from_utf8_lossy(received)
        );
        received.extend_from_slice(&buffer[..count]);
    }
}

fn payload(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i % 251) as u8).collect()
}

/// The `name=value` lines `/info` returns, as a map.
fn info(body: &str) -> std::collections::BTreeMap<String, String> {
    body.lines()
        .map(|line| {
            let (name, value) = line.split_once('=').unwrap();
            (name.to_owned(), value.to_owned())
        })
        .collect()
}

/// Parses `connections=N requests=N ...`.
fn report(text: &str) -> std::collections::BTreeMap<String, u64> {
    text.split(' ')
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap();
            (name.to_owned(), value.parse().unwrap())
        })
        .collect()
}

/// Host and the forwarding fields reach the handler; a spoofed
/// `X-Forwarded-For` is not believed; client connections and upstream
/// connections are both reused; TLS is terminated at the proxy.
fn forwarding_keep_alive_and_tls(kind: Kind) {
    let mut server = Server::start();
    let proxy = Proxy::start(kind, server.port);

    let (code, body) = fetch(&[
        "-H",
        "Host: app.example:8080",
        "-H",
        "X-Forwarded-For: 203.0.113.9",
        &proxy.url("/info"),
    ]);
    assert_eq!(code, "200", "{body}");
    let seen = info(&body);
    assert_eq!(seen["authority"], "app.example:8080", "{body}");
    assert_eq!(seen["Host"], "app.example:8080", "{body}");
    assert_eq!(seen["X-Forwarded-Proto"], "http", "{body}");
    assert_eq!(seen["peer"], "127.0.0.1", "{body}");
    // nginx appends the address it saw to the client's header; Caddy, which
    // trusts no proxy by default, replaces it.
    let forwarded = match proxy.kind {
        Kind::Nginx => "203.0.113.9, 127.0.0.2",
        Kind::Caddy => "127.0.0.2",
    };
    assert_eq!(seen["X-Forwarded-For"], forwarded, "{body}");
    assert_eq!(seen["client"], "127.0.0.2 http", "{body}");

    // Two URLs on one client connection: the second reuses it.
    let reuse = curl(&[
        "-w",
        "|%{http_code} %{num_connects}\n",
        "-o",
        "/dev/null",
        &proxy.url("/info"),
        "-o",
        "/dev/null",
        &proxy.url("/info"),
    ]);
    assert_eq!(String::from_utf8_lossy(&reuse.stdout), "|200 1\n|200 0\n");

    let tls = proxy.tls_args();
    let tls: Vec<&str> = tls.iter().map(String::as_str).collect();
    let mut args = tls.clone();
    let url = proxy.tls_url("/info");
    args.push(&url);
    let (code, body) = fetch(&args);
    assert_eq!(code, "200", "{body}");
    let seen = info(&body);
    assert_eq!(seen["X-Forwarded-Proto"], "https", "{body}");
    assert_eq!(seen["client"], "127.0.0.2 https", "{body}");
    assert_eq!(
        seen["authority"],
        format!("localhost:{}", proxy.tls),
        "{body}"
    );

    // Caddy offers HTTP/2 to clients and still speaks HTTP/1.1 to the server.
    let version = match proxy.kind {
        Kind::Nginx => "1.1",
        Kind::Caddy => "2",
    };
    let mut args = tls.clone();
    args.extend([
        "-w",
        "|%{http_version} %{num_connects}\n",
        "-o",
        "/dev/null",
    ]);
    args.extend([url.as_str(), "-o", "/dev/null", url.as_str()]);
    assert_eq!(
        String::from_utf8_lossy(&curl(&args).stdout),
        format!("|{version} 1\n|{version} 0\n")
    );

    let dir = tempfile::tempdir().unwrap();
    let upload = dir.path().join("upload");
    let sent = payload(700_000);
    std::fs::write(&upload, &sent).unwrap();
    let upload = format!("@{}", upload.display());
    let echo = proxy.tls_url("/echo");
    let mut args = tls.clone();
    args.extend(["--data-binary", &upload, &echo]);
    assert!(curl(&args).stdout == sent, "TLS upload");

    // Twelve requests came through the proxy over far fewer upstream
    // connections.
    for _ in 0..5 {
        assert_eq!(fetch(&[&proxy.url("/info")]).0, "200");
    }
    let counts = report(&server.terminate());
    assert_eq!(counts["requests"], 12, "{counts:?}");
    assert!(counts["connections"] <= 3, "{counts:?}");
    for name in ["handler_errors", "protocol_errors", "timeouts", "rejected"] {
        assert_eq!(counts[name], 0, "{name}: {counts:?}");
    }
}

/// Sized, chunked and `100-continue` uploads and large downloads, through the
/// buffering location and the streaming one (nginx `/stream/`; Caddy streams
/// both); a streamed response reaches the client before it ends.
fn bodies_and_continue(kind: Kind) {
    let mut server = Server::start();
    let proxy = Proxy::start(kind, server.port);
    let dir = tempfile::tempdir().unwrap();
    let upload = dir.path().join("upload");
    let sent = payload(700_000);
    std::fs::write(&upload, &sent).unwrap();
    let upload = format!("@{}", upload.display());

    for prefix in ["", "/stream"] {
        let echo = proxy.url(&format!("{prefix}/echo"));
        for (extra, label) in [
            (vec![], "sized"),
            (vec!["-H", "Transfer-Encoding: chunked"], "chunked"),
            (
                vec!["-H", "Expect: 100-continue", "--expect100-timeout", "30"],
                "continue",
            ),
        ] {
            let mut args = vec!["--data-binary", upload.as_str()];
            args.extend(extra);
            args.push(&echo);
            assert!(curl(&args).stdout == sent, "{prefix} {label} upload");
        }
        let big = curl(&[&proxy.url(&format!("{prefix}/big"))]).stdout;
        assert!(big.len() == 1 << 20 && big.iter().all(|&byte| byte == b'x'));

        // The client waits for `100 Continue` before sending the body.
        let mut stream = connect(proxy.http);
        stream
            .write_all(
                format!(
                    "POST {prefix}/echo HTTP/1.1\r\nHost: localhost\r\nContent-Length: 11\r\n\
                     Expect: 100-continue\r\nConnection: close\r\n\r\n"
                )
                .as_bytes(),
            )
            .unwrap();
        let mut received = Vec::new();
        read_until(&mut stream, &mut received, "\r\n\r\n");
        let interim = String::from_utf8_lossy(&received).into_owned();
        assert!(
            interim.starts_with("HTTP/1.1 100 Continue\r\n"),
            "{interim}"
        );
        stream.write_all(b"hello world").unwrap();
        stream.read_to_end(&mut received).unwrap();
        let text = String::from_utf8_lossy(&received);
        // Caddy also relays the server's own `100 Continue`, a second interim
        // response, which clients must accept (RFC 9110, section 15.2).
        let mut last = text.as_ref();
        while last.starts_with("HTTP/1.1 100 ") {
            last = last.split_once("\r\n\r\n").unwrap().1;
        }
        assert!(
            last.starts_with("HTTP/1.1 200 OK\r\n") && last.ends_with("\r\n\r\nhello world"),
            "{prefix}: {text}"
        );
    }

    // The first part of a held response arrives before the handler lets it
    // end, which happens only on the request to /release.
    let mut stream = connect(proxy.http);
    stream
        .write_all(b"GET /stream/hold HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut received = Vec::new();
    read_until(&mut stream, &mut received, "first\n");
    assert!(!String::from_utf8_lossy(&received).contains("second"));
    assert_eq!(
        fetch(&[&proxy.url("/release")]),
        ("200".into(), "released".into())
    );
    stream.read_to_end(&mut received).unwrap();
    let text = String::from_utf8_lossy(&received);
    assert!(
        text.contains("second\n") && text.ends_with("0\r\n\r\n"),
        "{text}"
    );

    let counts = report(&server.terminate());
    for name in ["handler_errors", "protocol_errors", "timeouts", "rejected"] {
        assert_eq!(counts[name], 0, "{name}: {counts:?}");
    }
}

/// The proxy, not the server, waits out a slow request head; a slow upstream
/// gets the proxy's timeout answer; refusals and early answers to requests
/// whose bodies the server does not read reach the client whole, because
/// the server closes with a lingering close (issue #138), not a reset.
fn slow_clients_timeouts_and_refusals(kind: Kind) {
    let mut server = Server::start();
    let proxy = Proxy::start(kind, server.port);

    let mut slow = connect(proxy.http);
    slow.write_all(b"GET /info HTTP/1.1\r\nHost: x").unwrap();
    let started = Instant::now();
    assert_eq!(fetch(&[&proxy.url("/info")]).0, "200");
    let mut received = Vec::new();
    let _ = slow.read_to_end(&mut received);
    let waited = started.elapsed();
    let text = String::from_utf8_lossy(&received);
    assert!(
        waited >= Duration::from_millis(1500) && waited < Duration::from_secs(15),
        "{waited:?}"
    );
    // Both proxies close without an answer.
    assert!(text.is_empty(), "{text}");

    let start = Instant::now();
    let (code, _) = fetch(&[&proxy.url("/slow")]);
    assert_eq!(code, "504");
    assert!(
        start.elapsed() < Duration::from_secs(4),
        "{:?}",
        start.elapsed()
    );
    assert_eq!(fetch(&[&proxy.url("/info")]).0, "200");

    let dir = tempfile::tempdir().unwrap();
    // Over `max_body` (1 MiB) but under nginx's `client_max_body_size`.
    let large = dir.path().join("large");
    std::fs::write(&large, payload(2 << 20)).unwrap();
    let large = format!("@{}", large.display());
    // Under `max_body`; /early answers without reading it. Before the
    // lingering budget grew from 64 KiB to 16 MiB, these and the refusals
    // above often became `502`: the proxy was still sending when the server
    // closed.
    let unread = dir.path().join("unread");
    std::fs::write(&unread, payload(1000 << 10)).unwrap();
    let unread = format!("@{}", unread.display());
    for round in 0..5 {
        for prefix in ["", "/stream"] {
            for extra in [vec![], vec!["-H", "Transfer-Encoding: chunked"]] {
                for (path, upload, expected) in [
                    ("echo", &large, ("413", "Content Too Large\n")),
                    ("early", &unread, ("200", "early")),
                ] {
                    let url = proxy.url(&format!("{prefix}/{path}"));
                    let mut args = vec!["--data-binary", upload.as_str()];
                    args.extend(&extra);
                    args.push(&url);
                    let (code, body) = if proxy.kind == Kind::Caddy && path == "early" {
                        relayed(&args)
                    } else {
                        fetch(&args)
                    };
                    assert_eq!(
                        (code.as_str(), body.as_str()),
                        expected,
                        "round {round}: {prefix}/{path} {extra:?}"
                    );
                }
            }
        }
    }
    assert_eq!(fetch(&[&proxy.url("/info")]).0, "200");

    let counts = report(&server.terminate());
    // The partial head never reached the server.
    assert_eq!(counts["timeouts"], 0, "{counts:?}");
}

/// `SIGTERM` while a response streams through the proxy: the response ends
/// whole, the server exits with its report, and the proxy then answers
/// `502` itself.
fn shutdown_drains_through_the_proxy(kind: Kind) {
    let mut server = Server::start();
    let proxy = Proxy::start(kind, server.port);
    assert_eq!(fetch(&[&proxy.url("/info")]).0, "200");
    let mut stream = connect(proxy.http);
    stream
        .write_all(b"GET /stream/tick HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut received = Vec::new();
    read_until(&mut stream, &mut received, "tick\n");
    let counts = report(&server.terminate());
    stream.read_to_end(&mut received).unwrap();
    let text = String::from_utf8_lossy(&received);
    assert!(text.starts_with("HTTP/1.1 200 OK\r\n"), "{text}");
    assert!(
        text.contains("bye\n") && text.ends_with("0\r\n\r\n"),
        "{text}"
    );
    assert_eq!(counts["requests"], 2, "{counts:?}");
    assert_eq!(counts["cancelled"], 0, "{counts:?}");
    assert_eq!(fetch(&[&proxy.url("/info")]).0, "502");
}

macro_rules! behind {
    ($kind:expr_2021, $($test:ident => $scenario:ident),* $(,)?) => {
        $(
            #[test]
            fn $test() {
                if enabled(stringify!($test)) {
                    $scenario($kind);
                }
            }
        )*
    };
}

behind!(
    Kind::Nginx,
    nginx_forwarding_keep_alive_and_tls => forwarding_keep_alive_and_tls,
    nginx_bodies_and_continue => bodies_and_continue,
    nginx_slow_clients_timeouts_and_refusals => slow_clients_timeouts_and_refusals,
    nginx_shutdown_drains => shutdown_drains_through_the_proxy,
);

behind!(
    Kind::Caddy,
    caddy_forwarding_keep_alive_and_tls => forwarding_keep_alive_and_tls,
    caddy_bodies_and_continue => bodies_and_continue,
    caddy_slow_clients_timeouts_and_refusals => slow_clients_timeouts_and_refusals,
    caddy_shutdown_drains => shutdown_drains_through_the_proxy,
);
