//! Production qualification suites for `std.http.server` (issue #103, B66):
//! a retained request corpus and its mutations, interoperability with curl
//! and `std.http.client`, slow clients, shutdown under sustained load,
//! descriptor and heap leaks, allocation failure on the accepting thread, a
//! relocated compiler building a JSON service, and an installed toolchain
//! building a CLI/data tool and a service as a package offline. Results and
//! the gates they cover are in `book/src/design/stdlib/http-qualification.md`.
//! Everything runs on loopback; no test needs a network.

mod support;
use support::build_file as build;

use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Command, Stdio};

/// Scripted in-memory transport and the corpus runner.
const SCRIPTED: &str = include_str!("fixtures/http_server/scripted.plenty");

/// Request corpus: expected statuses, then the escaped request bytes.
const CORPUS: &str = include_str!("fixtures/http_server/corpus.txt");

/// A loopback handler and client helpers.
const LOOPBACK: &str = r##"import std.bytes
import std.fs
import std.http
import std.http.server
import std.net
import std.path
import std.time
from std.bytes import Bytes
from std.http.server import Config, Exchange, Report, Router, ServerError
from std.net import NetError, TcpStream
from std.sync import CancellationToken

pub enum Route:
    # GET / -> "home"
    Home
    # POST /echo
    Echo
    # GET /big -> 1 MiB chunked
    Big
    # GET /huge -> 64 MiB with Content-Length
    Huge
    # GET /stop cancels `stop`
    Stop
    # GET /stream -> two chunks
    Stream

pub class App:
    pub stop: CancellationToken
    pub piece: Bytes

    pub def handle(self, route: &Route, exchange: &mut Exchange[TcpStream]) -> Result[(), std.bytes.BytesError | std.http.HttpError | AllocError]:
        match route:
            case Route.Home:
                exchange.send_text(200, "home")?
            case Route.Echo:
                mut body = std.bytes.empty()?
                exchange.read_all(&mut body)?
                exchange.send(200, "application/octet-stream", &body)?
            case Route.Big:
                exchange.start(std.http.response(200, "OK")?)?
                for index in range(16):
                    exchange.write(&self.piece, 0, self.piece.len())?
            case Route.Huge:
                mut head = std.http.response(200, "OK")?
                head.add("Content-Length", str.from(1024 * self.piece.len())?)?
                exchange.start(head)?
                for index in range(1024):
                    exchange.write(&self.piece, 0, self.piece.len())?
                exchange.end()?
            case Route.Stop:
                exchange.send_text(200, "stopping")?
                self.stop.cancel()
            case Route.Stream:
                exchange.start(std.http.response(200, "OK")?)?
                text = std.bytes.from_text("abcdef")?
                exchange.write(&text, 0, 2)?
                exchange.write(&text, 2, 6)?
        Ok(())

pub def app(stop: &CancellationToken) -> Result[App, std.bytes.BytesError]:
    mut piece = std.bytes.with_capacity(65536)?
    piece.resize(65536, 120u8)?
    Ok(App(stop.share(), piece))

pub def routes() -> Result[Router[Route], ServerError]:
    mut table = std.http.server.router[Route]()?
    table.add("GET", "/", Route.Home)?
    table.add("POST", "/echo", Route.Echo)?
    table.add("GET", "/big", Route.Big)?
    table.add("GET", "/huge", Route.Huge)?
    table.add("GET", "/stop", Route.Stop)?
    table.add("GET", "/stream", Route.Stream)?
    Ok(table)

pub def connect_to(port: u16, raw: &str) -> Result[TcpStream, NetError]:
    never = CancellationToken().unwrap()
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(10u32)).unwrap()
    mut stream = std.net.connect(&std.net.loopback_v4(port), &deadline, &never)?
    request = std.bytes.from_text(raw).unwrap()
    stream.write_all_before(&request, 0, request.len(), &deadline, &never)?
    Ok(stream)

pub def send(stream: &mut TcpStream, raw: &str) -> Result[(), NetError]:
    never = CancellationToken().unwrap()
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(10u32)).unwrap()
    request = std.bytes.from_text(raw).unwrap()
    stream.write_all_before(&request, 0, request.len(), &deadline, &never)?
    Ok(())

# Reads until the text ends with `marker`, or to the end when it is empty.
# Responses here are never pipelined, so a read cannot run past one.
pub def until(stream: &mut TcpStream, marker: &str) -> Result[str, NetError]:
    never = CancellationToken().unwrap()
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(10u32)).unwrap()
    mut out = std.bytes.empty().unwrap()
    while True:
        at = out.len()
        out.resize(at + 512, 0u8).unwrap()
        count = stream.read_before(&mut out, at, at + 512, &deadline, &never)?
        out.truncate(at + count).unwrap()
        if count == 0:
            break
        if marker.byte_len() > 0 and out.to_text().unwrap().endswith(marker):
            break
    Ok(out.to_text().unwrap().replace("\r\n", "|").unwrap())

pub def fetch(port: u16, raw: &str) -> Result[str, NetError]:
    mut stream = connect_to(port, raw)?
    until(&mut stream, "")

pub def millis_since(start: &std.time.Instant) -> u64:
    std.time.monotonic_now().saturating_duration_since(start).as_nanoseconds() // 1000000u64

# Open descriptors, counted through /proc. Measure with no other thread running.
pub def descriptors() -> Result[i64, IoError]:
    mut count = 0
    for entry in std.fs.entries(std.fs.read_dir(&std.path.from_text("/proc/self/fd")?)?):
        entry?
        count += 1
    Ok(count)

pub def counts(report: &Report) -> Result[str, AllocError]:
    mut text = "connections=".concat(str.from(report.connections)?)?
    text = text.concat(" requests=")?.concat(str.from(report.requests)?)?
    text = text.concat(" handler_errors=")?.concat(str.from(report.handler_errors)?)?
    text = text.concat(" protocol_errors=")?.concat(str.from(report.protocol_errors)?)?
    text = text.concat(" timeouts=")?.concat(str.from(report.timeouts)?)?
    text = text.concat(" cancelled=")?.concat(str.from(report.cancelled)?)?
    text = text.concat(" rejected=")?.concat(str.from(report.rejected)?)?
    Ok(text)
"##;

/// Runs a program to completion and returns its standard output, with CRLF
/// shown as `|` and test-hook markers removed.
fn output(source: &str) -> String {
    let (_dir, executable) = build(source);
    let output = Command::new(executable)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "status {:?}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .replace("\r\n", "|")
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn unescape(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        if c != '\\' {
            let mut buffer = [0; 4];
            out.extend_from_slice(c.encode_utf8(&mut buffer).as_bytes());
            continue;
        }
        let (_, kind) = chars.next().expect("escape");
        match kind {
            'r' => out.push(b'\r'),
            'n' => out.push(b'\n'),
            't' => out.push(b'\t'),
            '\\' => out.push(b'\\'),
            'x' => {
                let hex: String = (0..2).map(|_| chars.next().unwrap().1).collect();
                out.push(u8::from_str_radix(&hex, 16).unwrap());
            }
            '{' => {
                let end = at + text[at..].find('}').unwrap();
                let (count, piece) = text[at + 2..end].split_once('*').unwrap();
                let piece = unescape(piece);
                for _ in 0..count.parse::<usize>().unwrap() {
                    out.extend_from_slice(&piece);
                }
                while chars.peek().is_some_and(|(next, _)| *next <= end) {
                    chars.next();
                }
            }
            other => panic!("unknown escape \\{other}"),
        }
    }
    out
}

/// The corpus as (expected statuses, request bytes).
fn corpus() -> Vec<(String, Vec<u8>)> {
    CORPUS
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let (expected, raw) = line.split_once(" |").expect("case separator");
            (
                expected.to_owned(),
                unescape(raw.strip_prefix(' ').unwrap_or(raw)),
            )
        })
        .collect()
}

/// Serves each input on its own scripted connection, delivered `step` bytes
/// at a time, and returns the statuses written and the counts line per input.
fn replay(inputs: &[&[u8]], step: usize) -> Vec<(String, String)> {
    let mut blob = String::new();
    for input in inputs {
        assert!(!input.contains(&0xff));
        for byte in *input {
            blob.push_str(&format!("{byte:02x}"));
        }
        blob.push_str("ff");
    }
    let source =
        format!("{SCRIPTED}\ndef main() -> Result[(), std.bytes.BytesError | std.encoding.EncodingError | std.http.server.ServerError | AllocError | IoError]:\n    replay(\"{blob}\", {step})?\n    Ok(())\n");
    let text = output(&source);
    let cases: Vec<(String, String)> = text
        .split("--\n")
        .skip(1)
        .map(|case| {
            let statuses = statuses(case);
            let counts = case.trim_end().lines().last().unwrap_or("").to_owned();
            (statuses, counts)
        })
        .collect();
    assert_eq!(cases.len(), inputs.len(), "{text}");
    cases
}

/// The status of every response head in `text`, space-separated, or `-`.
fn statuses(text: &str) -> String {
    let mut found = Vec::new();
    for (at, _) in text.match_indices("HTTP/1.") {
        let rest = &text[at + "HTTP/1.".len()..];
        if rest.len() >= 6
            && (rest.starts_with("1 ") || rest.starts_with("0 "))
            && rest[2..5].bytes().all(|b| b.is_ascii_digit())
            && rest.as_bytes()[5] == b' '
        {
            found.push(rest[2..5].to_owned());
        }
    }
    if found.is_empty() {
        "-".to_owned()
    } else {
        found.join(" ")
    }
}

/// Every corpus case gets its recorded answer, whether its bytes arrive in
/// 64-byte reads or one byte at a time.
#[test]
fn corpus_requests_get_their_recorded_answers() {
    let cases = corpus();
    assert!(cases.len() >= 50);
    let inputs: Vec<&[u8]> = cases.iter().map(|(_, input)| input.as_slice()).collect();
    for step in [64, 1] {
        for ((expected, input), (actual, counts)) in cases.iter().zip(replay(&inputs, step)) {
            assert_eq!(
                &actual,
                expected,
                "step {step}: {:?}\n{counts}",
                String::from_utf8_lossy(input)
            );
        }
    }
}

/// Deterministic xorshift so that a failure names a reproducible case.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

/// Mutated corpus requests, delivered 7 bytes at a time, get only statuses
/// the server documents and never trap, hang, or stop the connection loop.
#[test]
fn mutated_corpus_requests_fail_cleanly() {
    let cases = corpus();
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let alphabet = b"\r\n :;,/?%0123456789aAfFzZ-\t\x00\x7f\x80Hh";
    let mut inputs: Vec<Vec<u8>> = Vec::new();
    for _ in 0..600 {
        let mut input = cases[rng.below(cases.len())].1.clone();
        for _ in 0..1 + rng.below(4) {
            if input.is_empty() {
                input.push(b'G');
            }
            let at = rng.below(input.len());
            match rng.below(4) {
                0 => input[at] = alphabet[rng.below(alphabet.len())],
                1 => {
                    input.remove(at);
                }
                2 => input.insert(at, alphabet[rng.below(alphabet.len())]),
                _ => {
                    let stop = (at + 1 + rng.below(8)).min(input.len());
                    let copy = input[at..stop].to_vec();
                    input.splice(at..at, copy);
                }
            }
        }
        inputs.push(input);
    }
    let refs: Vec<&[u8]> = inputs.iter().map(Vec::as_slice).collect();
    let allowed = [
        "100", "200", "400", "404", "405", "413", "431", "500", "501", "505",
    ];
    for (input, (actual, counts)) in refs.iter().zip(replay(&refs, 7)) {
        assert!(
            actual == "-" || actual.split(' ').all(|status| allowed.contains(&status)),
            "{:?} -> {actual}",
            String::from_utf8_lossy(input)
        );
        assert!(counts.starts_with("requests="), "{counts}");
    }
}

/// A running server program: its port, then its output after it exits.
struct Served {
    child: std::process::Child,
    port: u16,
    _dir: tempfile::TempDir,
}

impl Served {
    fn start(main: &str) -> Served {
        let (dir, executable) = build(&format!("{LOOPBACK}{main}"));
        let mut child = Command::new(executable)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.as_mut().unwrap())
            .read_line(&mut line)
            .unwrap();
        let port = line
            .trim()
            .strip_prefix("port ")
            .unwrap_or_else(|| panic!("no port line: {line:?}"))
            .parse()
            .unwrap();
        Served {
            child,
            port,
            _dir: dir,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    fn finish(mut self) -> String {
        let mut rest = String::new();
        self.child
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut rest)
            .unwrap();
        let status = self.child.wait().unwrap();
        assert!(status.success(), "{status:?}\n{rest}");
        rest.trim_end().to_owned()
    }
}

/// Serves until a request to `/stop`, printing the port first and the counts last.
const SERVE_UNTIL_STOPPED: &str = r##"
def main() -> Result[(), std.bytes.BytesError | std.net.NetError | std.http.server.ServerError | AllocError | IoError]:
    table = routes()?
    stop = CancellationToken()?
    site = app(&stop)?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 64)?
    print("port ".concat(str.from(i64(listener.local_address()?.port))?)?)?
    report = std.http.server.serve(listener, &table, &site, &std.http.server.config(), &stop)?
    print(counts(&report)?)?
    Ok(())
"##;

fn curl(args: &[&str]) -> std::process::Output {
    let output = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--max-time",
            "20",
            "--noproxy",
            "*",
        ])
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

/// curl, an independent HTTP/1.1 client, reuses connections, sends sized,
/// chunked and `100-continue` bodies, speaks HTTP/1.0, and decodes chunked
/// and sized responses from the server. Skipped when curl is not installed.
#[test]
fn curl_interoperates() {
    if Command::new("curl").arg("--version").output().is_err() {
        eprintln!("curl not found; skipping");
        return;
    }
    let server = Served::start(SERVE_UNTIL_STOPPED);
    let dir = tempfile::tempdir().unwrap();
    let upload = dir.path().join("upload");
    let payload: Vec<u8> = (0..700_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(&upload, &payload).unwrap();
    let upload = format!("@{}", upload.display());

    // Two URLs on one connection: the second reuses it.
    let reuse = curl(&[
        "-w",
        "|%{http_code} %{num_connects}\n",
        &server.url("/"),
        &server.url("/"),
    ]);
    assert_eq!(
        String::from_utf8_lossy(&reuse.stdout),
        "home|200 1\nhome|200 0\n"
    );
    for (extra, label) in [
        (vec![], "sized"),
        (vec!["-H", "Transfer-Encoding: chunked"], "chunked"),
        (vec!["-H", "Expect: 100-continue"], "continue"),
    ] {
        let mut args = vec!["--data-binary", &upload];
        args.extend(extra);
        let url = server.url("/echo");
        args.push(&url);
        assert!(curl(&args).stdout == payload, "{label} upload");
    }
    let head = curl(&["--head", &server.url("/")]);
    let head = String::from_utf8_lossy(&head.stdout).to_ascii_lowercase();
    assert!(head.starts_with("http/1.1 200 ok\r\n") && head.contains("content-length: 4\r\n"));
    let old = curl(&["--http1.0", "-i", &server.url("/stream")]);
    let old = String::from_utf8_lossy(&old.stdout);
    // An HTTP/1.0 client gets an HTTP/1.1 status line and a close-delimited body.
    assert_eq!(old, "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nabcdef");
    assert_eq!(curl(&[&server.url("/stream")]).stdout, b"abcdef");
    assert_eq!(curl(&[&server.url("/big")]).stdout.len(), 1 << 20);
    assert_eq!(curl(&[&server.url("/huge")]).stdout.len(), 64 << 20);
    let missing = curl(&[
        "-w",
        "%{http_code}",
        "-o",
        "/dev/null",
        &server.url("/nope"),
    ]);
    assert_eq!(missing.stdout, b"404");
    let allow = curl(&["-i", "-X", "DELETE", &server.url("/")]);
    let allow = String::from_utf8_lossy(&allow.stdout);
    assert!(
        allow.starts_with("HTTP/1.1 405 ") && allow.contains("\r\nAllow: GET, HEAD\r\n"),
        "{allow}"
    );
    curl(&[&server.url("/stop")]);
    assert_eq!(
        server.finish(),
        "connections=12 requests=13 handler_errors=0 protocol_errors=0 timeouts=0 cancelled=0 rejected=0"
    );
}

/// curl over TLS against `std.http.tls.serve`: certificate verification
/// against the test CA, connection reuse and a 700 KB upload. Skipped when
/// curl is not installed.
#[test]
fn curl_interoperates_over_tls() {
    if Command::new("curl").arg("--version").output().is_err() {
        eprintln!("curl not found; skipping");
        return;
    }
    let fixtures = format!("{}/tests/fixtures/tls", env!("CARGO_MANIFEST_DIR"));
    let main = r##"import std.http.tls
import std.tls
from std.http.tls import TlsTransport

pub class Secure:
    pub stop: CancellationToken

    pub def handle(self, route: &Route, exchange: &mut Exchange[TlsTransport]) -> Result[(), std.bytes.BytesError | std.http.HttpError]:
        match route:
            case Route.Echo:
                mut body = std.bytes.empty()?
                exchange.read_all(&mut body)?
                exchange.send(200, "application/octet-stream", &body)?
            case Route.Stop:
                exchange.send_text(200, "stopping")?
                self.stop.cancel()
            case _:
                exchange.send_text(200, "secure")?
        Ok(())

def main() -> Result[(), std.net.NetError | std.http.server.ServerError | std.tls.TlsError | AllocError | IoError]:
    table = routes()?
    stop = CancellationToken()?
    tls = std.tls.server_config(&read_text("FIXTURES/leaf.pem")?, &read_text("FIXTURES/leaf.key")?)?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 64)?
    print("port ".concat(str.from(i64(listener.local_address()?.port))?)?)?
    report = std.http.tls.serve(listener, tls, &table, &Secure(stop.share()), &std.http.server.config(), &stop)?
    print(counts(&report)?)?
    Ok(())
"##
    .replace("FIXTURES", &fixtures);
    let server = Served::start(&main);
    let ca = format!("{fixtures}/ca.pem");
    let url = |path: &str| format!("https://localhost:{}{path}", server.port);
    let resolve = format!("localhost:{}:127.0.0.1", server.port);
    let reuse = curl(&[
        "--cacert",
        &ca,
        "--resolve",
        &resolve,
        "-w",
        "|%{http_version} %{num_connects}\n",
        &url("/"),
        &url("/"),
    ]);
    assert_eq!(
        String::from_utf8_lossy(&reuse.stdout),
        "secure|1.1 1\nsecure|1.1 0\n"
    );
    let dir = tempfile::tempdir().unwrap();
    let upload = dir.path().join("upload");
    let payload: Vec<u8> = (0..700_000u32).map(|i| (i % 253) as u8).collect();
    std::fs::write(&upload, &payload).unwrap();
    let echoed = curl(&[
        "--cacert",
        &ca,
        "--resolve",
        &resolve,
        "--data-binary",
        &format!("@{}", upload.display()),
        &url("/echo"),
    ]);
    assert!(echoed.stdout == payload);
    // Without the test CA, curl refuses the certificate.
    let refused = Command::new("curl")
        .args([
            "--silent",
            "--max-time",
            "20",
            "--resolve",
            &resolve,
            &url("/"),
        ])
        .output()
        .unwrap();
    assert!(!refused.status.success());
    curl(&["--cacert", &ca, "--resolve", &resolve, &url("/stop")]);
    // The refused client is not counted as a protocol error whether it
    // finishes the handshake and closes (OpenSSL 3) or sends a fatal alert
    // mid-handshake (OpenSSL 1.1.1, issue #152).
    assert_eq!(
        server.finish(),
        "connections=4 requests=4 handler_errors=0 protocol_errors=0 timeouts=0 cancelled=0 rejected=0"
    );
}

/// `std.http.client` against the server: pooled keep-alive requests, a
/// streamed chunked upload, sized and chunked downloads, and `HEAD`.
#[test]
fn std_http_client_interoperates() {
    // Exact reuse counts require idle connections to outlive the entire
    // request sequence, including the separate client's 64 MiB download.
    // The server's default five-second idle timeout can expire on slow CI.
    let main = r##"import std.http.client
import std.io

const request_budget_seconds: u32 = 20u32

def visit(port: u16, stop: CancellationToken) -> Result[(), std.bytes.BytesError | std.http.client.ClientError | RangeError | AllocError | IoError]:
    never = CancellationToken()?
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(request_budget_seconds))?
    base = "http://127.0.0.1:".concat(&str.from(i64(port))?)?
    mut client_config = std.http.client.config()?
    client_config.idle_timeout = std.time.seconds(request_budget_seconds)
    mut client = std.http.client.client(client_config)?
    for index in range(3):
        home = client.get(&base.concat("/")?, &deadline, &never)?
        print(home.status, home.body.to_text()?)?
    mut payload = std.bytes.with_capacity(300000)?
    payload.resize(300000, 0u8)?
    for index in range(300000):
        payload[index] = u8(index % 251)
    expected = payload.copy()?
    mut source = std.io.cursor(payload)
    post = std.http.client.request("POST", &base.concat("/echo")?)?
    mut response = client.send_from(post, &mut source, &std.http.client.chunked(), &deadline, &never)?
    mut echoed = std.bytes.empty()?
    response.read_all(&mut echoed)?
    client.finish(response)
    print(echoed == expected)?
    big = client.get(&base.concat("/big")?, &deadline, &never)?
    print(big.body.len())?
    mut config = std.http.client.config()?
    config.limits.max_body = 67108864
    mut large = std.http.client.client(config)?
    huge = large.get(&base.concat("/huge")?, &deadline, &never)?
    print(huge.body.len())?
    head = client.fetch(std.http.client.request("HEAD", &base.concat("/")?)?, &deadline, &never)?
    print(head.status, head.body.len())?
    stats = client.stats()
    print(stats.opened, stats.reused)?
    stop.cancel()
    Ok(())

def main() -> Result[(), std.bytes.BytesError | std.net.NetError | std.http.server.ServerError | ThreadError | AllocError | IoError]:
    table = routes()?
    stop = CancellationToken()?
    site = app(&stop)?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 64)?
    port = listener.local_address()?.port
    mut config = std.http.server.config()
    config.idle_timeout = std.time.seconds(request_budget_seconds)
    with spawn(visit, port, stop.share())? as task:
        report = std.http.server.serve(listener, &table, &site, &config, &stop)?
        print(counts(&report)?)?
    Ok(())
"##;
    assert_eq!(
        output(&format!("{LOOPBACK}{main}")),
        "200 home\n200 home\n200 home\nTrue\n1048576\n67108864\n200 0\n1 5\nconnections=2 requests=7 handler_errors=0 protocol_errors=0 timeouts=0 cancelled=0 rejected=0"
    );
}

/// Slow request heads (issue #132): six connections that send part of a head
/// and stall wait on the accepting thread, not on the two workers, so a
/// well-behaved request is answered at once instead of after three head
/// deadlines; each slow one gets `408` at its own deadline.
#[test]
fn slow_heads_do_not_hold_workers() {
    let main = r##"
def client(port: u16, stop: CancellationToken) -> ():
    begin = std.time.monotonic_now()
    mut slow = list[TcpStream].new().unwrap()
    for index in range(6):
        slow.append(connect_to(port, "GET / HTTP/1.1\r\nHost: x").unwrap()).unwrap()
    std.time.sleep(&std.time.milliseconds(50u32))
    start = std.time.monotonic_now()
    print(fetch(port, "GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap().endswith("home")).unwrap()
    print(millis_since(&start) < 500u64).unwrap()
    for index in range(6):
        print(until(&mut slow[index], "").unwrap().startswith("HTTP/1.1 408 ")).unwrap()
    # Each deadline runs from its own first byte, so all six end in one round.
    total = millis_since(&begin)
    print(total >= 1000u64, total < 2000u64).unwrap()
    stop.cancel()

def main() -> Result[(), std.bytes.BytesError | std.net.NetError | std.http.server.ServerError | ThreadError | AllocError | IoError]:
    table = routes()?
    stop = CancellationToken()?
    site = app(&stop)?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 64)?
    port = listener.local_address()?.port
    mut config = std.http.server.config()
    config.workers = 2
    config.head_timeout = std.time.milliseconds(1000u32)
    with spawn(client, port, stop.share())? as task:
        report = std.http.server.serve(listener, &table, &site, &config, &stop)?
        print(counts(&report)?)?
    Ok(())
"##;
    assert_eq!(
        output(&format!("{LOOPBACK}{main}")),
        "True\nTrue\nTrue\nTrue\nTrue\nTrue\nTrue\nTrue\nTrue True\nconnections=7 requests=1 handler_errors=0 protocol_errors=0 timeouts=6 cancelled=0 rejected=0"
    );
}

/// A client that stops reading a 64 MiB response is cut off at the write
/// deadline and frees its worker; a client trickling a body is cut off at
/// the body deadline with `408`; a pipelined partial head stalls into a `408`.
#[test]
fn slow_readers_and_senders_are_cut_off_at_their_deadlines() {
    let main = r##"
def client(port: u16, stop: CancellationToken) -> ():
    start = std.time.monotonic_now()
    mut stalled = connect_to(port, "GET /huge HTTP/1.1\r\nHost: x\r\n\r\n").unwrap()
    std.time.sleep(&std.time.milliseconds(100u32))
    # The only worker is free again once the write deadline has passed.
    print(fetch(port, "GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap().endswith("home")).unwrap()
    print(millis_since(&start) < 5000u64).unwrap()
    drop(stalled)
    mut trickle = connect_to(port, "POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 100\r\n\r\nabc").unwrap()
    print(str.repr(until(&mut trickle, "").unwrap()).unwrap()).unwrap()
    mut partial = connect_to(port, "GET / HTTP/1.1\r\nHost: x\r\n\r\nGET / HT").unwrap()
    print(until(&mut partial, "").unwrap()).unwrap()
    stop.cancel()

def main() -> Result[(), std.bytes.BytesError | std.net.NetError | std.http.server.ServerError | ThreadError | AllocError | IoError]:
    table = routes()?
    stop = CancellationToken()?
    site = app(&stop)?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 64)?
    port = listener.local_address()?.port
    mut config = std.http.server.config()
    config.workers = 1
    config.write_timeout = std.time.milliseconds(300u32)
    config.body_timeout = std.time.milliseconds(300u32)
    config.head_timeout = std.time.milliseconds(300u32)
    with spawn(client, port, stop.share())? as task:
        report = std.http.server.serve(listener, &table, &site, &config, &stop)?
        print(counts(&report)?)?
    Ok(())
"##;
    assert_eq!(
        output(&format!("{LOOPBACK}{main}")),
        r#"True
True
"HTTP/1.1 408 Request Timeout|Content-Type: text/plain; charset=utf-8|Content-Length: 16|Connection: close||Request Timeout\n"
HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 4||homeHTTP/1.1 408 Request Timeout|Content-Type: text/plain; charset=utf-8|Content-Length: 16|Connection: close||Request Timeout

connections=4 requests=4 handler_errors=1 protocol_errors=1 timeouts=3 cancelled=0 rejected=0"#
    );
}

/// Refusals of requests the server has not read to the end (issue #138):
/// `431` for a long field, `413` for a declared body over `max_body`, `400`
/// for a malformed head, and `400` from a handler whose body read failed,
/// each followed by input the server never reads. The server ends its output
/// and discards input before closing, so the client reads the whole answer
/// and a clean end instead of a reset, both when the connection lingers
/// parked on the accepting thread and when it lingers on its worker.
#[test]
fn refusals_end_cleanly_instead_of_resetting() {
    use std::io::Write;
    let filler = |byte: u8, size: usize| vec![byte; size];
    let cases: Vec<(&str, Vec<u8>)> = vec![
        (
            "HTTP/1.1 431 ",
            [
                &b"GET / HTTP/1.1\r\nHost: x\r\nX-Long: "[..],
                &filler(b'a', 20000),
                b"\r\n\r\n",
            ]
            .concat(),
        ),
        (
            "HTTP/1.1 413 ",
            [
                &b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 99999999\r\n\r\n"[..],
                &filler(b'b', 40000),
            ]
            .concat(),
        ),
        (
            "HTTP/1.1 400 ",
            [
                &b"GET / HTTP/1.1\r\nHost: x\r\nBad Header\r\n\r\n"[..],
                &filler(b'c', 40000),
            ]
            .concat(),
        ),
        (
            "HTTP/1.1 400 ",
            [
                &b"POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n"[..],
                &filler(b'd', 40000),
            ]
            .concat(),
        ),
    ];
    let serve = "    report = std.http.server.serve(listener, &table, &site, &std.http.server.config(), &stop)?";
    assert!(SERVE_UNTIL_STOPPED.contains(serve));
    let on_worker = SERVE_UNTIL_STOPPED.replace(
        serve,
        "    mut config = std.http.server.config()\n    config.idle_connections = 0\n    report = std.http.server.serve(listener, &table, &site, &config, &stop)?",
    );
    for main in [SERVE_UNTIL_STOPPED, &on_worker] {
        let server = Served::start(main);
        for (status, request) in &cases {
            for _ in 0..5 {
                let mut stream = std::net::TcpStream::connect(("127.0.0.1", server.port)).unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                    .unwrap();
                stream.write_all(request).unwrap();
                let mut answer = Vec::new();
                stream
                    .read_to_end(&mut answer)
                    .unwrap_or_else(|error| panic!("{status}: {error}"));
                let answer = String::from_utf8_lossy(&answer);
                assert!(
                    answer.starts_with(status) && answer.contains("\r\nConnection: close\r\n"),
                    "{answer}"
                );
            }
        }
        curl(&[&server.url("/stop")]);
        assert_eq!(
            server.finish(),
            "connections=21 requests=11 handler_errors=0 protocol_errors=20 timeouts=0 cancelled=0 rejected=0"
        );
    }
}

/// The same over TLS: after a `431` the server sends `close_notify` and
/// discards the rest of the request, so the client reads the answer and
/// then `close_notify`, not a truncated stream.
#[test]
fn tls_refusals_end_with_close_notify() {
    let fixtures = format!("{}/tests/fixtures/tls", env!("CARGO_MANIFEST_DIR"));
    let main = r##"import std.http.tls
import std.tls
from std.http.tls import TlsTransport

pub class Secure:
    pub def handle(self, route: &Route, exchange: &mut Exchange[TlsTransport]) -> Result[(), std.http.HttpError]:
        exchange.send_text(200, "secure")?
        Ok(())

def visit(port: u16, stop: CancellationToken) -> Result[(), std.bytes.BytesError | std.net.NetError | RangeError | std.tls.TlsError | AllocError | IoError]:
    never = CancellationToken()?
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(10u32))?
    config = std.tls.client_config(&std.tls.roots_from_pem(&read_text("FIXTURES/ca.pem")?)?)?
    for index in range(3):
        tcp = std.net.connect(&std.net.loopback_v4(port), &deadline, &never)?
        mut tls = std.tls.connect(tcp, &config, "localhost", &deadline, &never)?
        mut request = std.bytes.from_text("GET / HTTP/1.1\r\nHost: localhost\r\nX-Long: ")?
        request.resize(request.len() + 20000, 97u8)?
        request.extend(&std.bytes.from_text("\r\n\r\n")?)?
        # A failed write still reads, so `stop` is always cancelled.
        drop(tls.write_all_before(&request, 0, request.len(), &deadline, &never))
        mut answer = std.bytes.empty()?
        mut ending = "close_notify"
        while True:
            at = answer.len()
            answer.resize(at + 512, 0u8)?
            match tls.read_before(&mut answer, at, at + 512, &deadline, &never):
                case Ok(count):
                    answer.truncate(at + count)?
                    if count == 0:
                        break
                case Err(_):
                    answer.truncate(at)?
                    ending = "failed"
                    break
        print(answer.to_text()?.startswith("HTTP/1.1 431 "), ending)?
    stop.cancel()
    Ok(())

def main() -> Result[(), std.net.NetError | std.http.server.ServerError | ThreadError | std.tls.TlsError | AllocError | IoError]:
    table = routes()?
    stop = CancellationToken()?
    tls = std.tls.server_config(&read_text("FIXTURES/leaf.pem")?, &read_text("FIXTURES/leaf.key")?)?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 64)?
    port = listener.local_address()?.port
    with spawn(visit, port, stop.share())? as task:
        report = std.http.tls.serve(listener, tls, &table, &Secure(), &std.http.server.config(), &stop)?
        print(counts(&report)?)?
    Ok(())
"##
    .replace("FIXTURES", &fixtures);
    assert_eq!(
        output(&format!("{LOOPBACK}{main}")),
        "True close_notify\nTrue close_notify\nTrue close_notify\nconnections=3 requests=0 handler_errors=0 protocol_errors=3 timeouts=0 cancelled=0 rejected=0"
    );
}

/// Six clients send keep-alive requests as fast as they can while shutdown
/// begins. Every response a client receives is whole, the server's request
/// count matches the responses received, `serve` returns within the drain
/// deadline, and no descriptor or heap allocation survives the round. A
/// second round mixes in faults: bad heads, silent clients and rejections.
#[test]
fn shutdown_under_sustained_load_loses_no_response_or_resource() {
    let main = r##"
# `late` counts whole responses read after the stop signal: requests were
# still in flight when shutdown began.
def hammer(port: u16, stop: CancellationToken) -> tuple[i64, i64, i64]:
    mut whole = 0
    mut broken = 0
    mut late = 0
    while not stop.is_cancelled():
        match connect_to(port, "GET / HTTP/1.1\r\nHost: x\r\n\r\n"):
            case Ok(stream):
                mut conn = stream
                while True:
                    text = match until(&mut conn, "home"):
                        case Ok(found):
                            found
                        case Err(_):
                            ""
                    # The last response on a connection adds `Connection: close`.
                    if text.startswith("HTTP/1.1 200 OK|") and text.endswith("||home"):
                        whole += 1
                        if stop.is_cancelled():
                            late += 1
                    elif text != "":
                        broken += 1
                        break
                    else:
                        break
                    if send(&mut conn, "GET / HTTP/1.1\r\nHost: x\r\n\r\n").is_err():
                        break
            case Err(_):
                break
    (whole, broken, late)

def stopper(stop: CancellationToken) -> ():
    std.time.sleep(&std.time.milliseconds(500u32))
    stop.cancel()

def faults(port: u16, stop: CancellationToken) -> ():
    for round in range(20):
        drop(fetch(port, "GET / HTTP/1.1\r\nBad Header\r\n\r\n").unwrap())
        drop(fetch(port, "GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap())
        drop(connect_to(port, "GET / HT").unwrap())
    mut silent = list[TcpStream].new().unwrap()
    for index in range(4):
        silent.append(connect_to(port, "").unwrap()).unwrap()
    # idle_connections is 2: at least two of these four are turned away.
    std.time.sleep(&std.time.milliseconds(400u32))
    stop.cancel()

def load_round() -> Result[(), std.bytes.BytesError | std.net.NetError | std.http.server.ServerError | ThreadError | AllocError | IoError]:
    table = routes()?
    mut config = std.http.server.config()
    config.drain_timeout = std.time.milliseconds(2000u32)
    stop = CancellationToken()?
    site = app(&stop)?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 128)?
    port = listener.local_address()?.port
    mut summary = ""
    mut prompt = False
    mut whole = 0
    mut broken = 0
    mut late = 0
    mut fewest = -1
    start = std.time.monotonic_now()
    with spawn(stopper, stop.share())? as timer, spawn(hammer, port, stop.share())? as a, spawn(hammer, port, stop.share())? as b, spawn(hammer, port, stop.share())? as c, spawn(hammer, port, stop.share())? as d, spawn(hammer, port, stop.share())? as e, spawn(hammer, port, stop.share())? as f:
        report = std.http.server.serve(listener, &table, &site, &config, &stop)?
        summary = counts(&report)?
        # Stop comes at 500 ms; the drain deadline is 2 s after that.
        prompt = millis_since(&start) < 3000u64
        for pair in [a.join(), b.join(), c.join(), d.join(), e.join(), f.join()]?:
            (got, lost, after) = pair
            whole += got
            broken += lost
            late += after
            if fewest < 0 or got < fewest:
                fewest = got
    print(prompt, broken, whole, fewest, late)?
    print(summary)?
    Ok(())

def fault_round() -> Result[(), std.bytes.BytesError | std.net.NetError | std.http.server.ServerError | ThreadError | AllocError | IoError]:
    table = routes()?
    mut config = std.http.server.config()
    config.idle_connections = 2
    config.idle_timeout = std.time.milliseconds(200u32)
    config.head_timeout = std.time.milliseconds(200u32)
    stop = CancellationToken()?
    site = app(&stop)?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 128)?
    port = listener.local_address()?.port
    with spawn(faults, port, stop.share())? as task:
        report = std.http.server.serve(listener, &table, &site, &config, &stop)?
        # Admission turns a new connection away once two are parked, without
        # reading what it sent, so under load any fault connection may be
        # rejected instead of answered: each of the 60 counts once as a
        # request, a protocol error or a rejection, and 2 to 4 silent
        # clients are rejected.
        print(report.connections + report.rejected, report.requests + report.protocol_errors + report.rejected, report.requests, report.protocol_errors, report.handler_errors, report.rejected)?
    Ok(())

# Each round's values are dropped when it returns, so the heap checkpoint
# after it sees only what the round leaked.
def main() -> Result[(), std.bytes.BytesError | std.net.NetError | std.http.server.ServerError | ThreadError | AllocError | IoError]:
    before = descriptors()?
    load_round()?
    print(descriptors()? - before)?
    print("__test_small_live_heap__")?
    fault_round()?
    print(descriptors()? - before)?
    print("__test_small_live_heap__")?
    Ok(())
"##;
    let text = output(&format!("{LOOPBACK}{main}"));
    let lines: Vec<&str> = text.lines().collect();
    let load: Vec<&str> = lines[0].split(' ').collect();
    assert_eq!(&load[..2], ["True", "0"], "{text}");
    let whole: u64 = load[2].parse().unwrap();
    // Load was sustained into shutdown without a machine-speed constant: every
    // client reused its connection, and some responses arrived after the stop.
    let fewest: u64 = load[3].parse().unwrap();
    let late: u64 = load[4].parse().unwrap();
    assert!(fewest >= 2, "{text}");
    assert!(late >= 1, "{text}");
    assert!(
        lines[1].ends_with(&format!(
            " requests={whole} handler_errors=0 protocol_errors=0 timeouts=0 cancelled=0 rejected=0"
        )),
        "{text}"
    );
    assert_eq!(lines[2], "0", "{text}");
    assert_eq!(lines[4], "0", "{text}");
    let fault: Vec<u64> = lines[3]
        .split(' ')
        .map(|n| n.parse().unwrap_or_else(|_| panic!("{text}")))
        .collect();
    let [connections, accounted, requests, protocol_errors, handler_errors, rejected] = fault[..]
    else {
        panic!("{text}");
    };
    assert_eq!(connections, 64, "{text}");
    assert!((62..=64).contains(&accounted), "{text}");
    assert!(requests <= 20 && protocol_errors <= 40, "{text}");
    assert_eq!(handler_errors, 0, "{text}");
    assert!(rejected >= 2, "{text}");
}

/// A handler's `Err` is answered with `500` and counted, and the server goes
/// on; a fatal trap in a handler (here an index out of range) ends the whole
/// process at once with a located message, cutting off every connection.
#[test]
fn a_handler_error_is_answered_but_a_trap_ends_the_process() {
    let main = r##"
pub enum Trial:
    Fail
    # Reads `values[index]`.
    Trap(i64)

pub enum Deliberate:
    Fail

pub class Fragile:
    pub def handle(self, route: &Trial, exchange: &mut Exchange[TcpStream]) -> Result[(), Deliberate | std.http.HttpError | AllocError]:
        match route:
            case Trial.Fail:
                return Err(Deliberate.Fail)
            case Trial.Trap(index):
                values = [1, 2, 3]?
                exchange.send_text(200, &str.from(values[*index])?)?
        Ok(())

def client(port: u16, stop: CancellationToken) -> ():
    print(fetch(port, "GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap()).unwrap()
    drop(fetch(port, "GET /trap HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"))
    print("unreachable").unwrap()

def main() -> Result[(), std.net.NetError | std.http.server.ServerError | ThreadError | AllocError]:
    mut table = std.http.server.router[Trial]()?
    table.add("GET", "/", Trial.Fail)?
    table.add("GET", "/trap", Trial.Trap(6))?
    stop = CancellationToken()?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    with spawn(client, port, stop.share())? as task:
        drop(std.http.server.serve(listener, &table, &Fragile(), &std.http.server.config(), &stop)?)
    Ok(())
"##;
    let (_dir, executable) = build(&format!("{LOOPBACK}{main}"));
    let ran = Command::new(executable).output().unwrap();
    let stdout = String::from_utf8_lossy(&ran.stdout).replace("\r\n", "|");
    let stderr = String::from_utf8_lossy(&ran.stderr);
    assert!(!ran.status.success());
    assert_eq!(
        stdout,
        "HTTP/1.1 500 Internal Server Error|Content-Type: text/plain; charset=utf-8|Content-Length: 22|Connection: close||Internal Server Error\n\n"
    );
    assert!(stderr.contains("index"), "{stderr}");
}

/// Allocation failure at every point on `serve`'s own thread (setup of
/// channels, selector and workers, then accepting, parking and admitting)
/// either fails `serve` or loses that connection. It never traps or leaks
/// (the allocation-checks runtime checks the balance at exit), and once
/// allocation succeeds the request is answered. Injected failures apply to
/// the marking thread only; request handling on a worker is the same code
/// that `test_std_http_server.rs` fails through `serve_connection`.
#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_on_the_accepting_thread_is_survived() {
    let main = r##"
pub class Once:
    pub stop: CancellationToken

    pub def handle(self, route: &Route, exchange: &mut Exchange[TcpStream]) -> Result[(), std.http.HttpError]:
        exchange.send_text(200, "home")?
        self.stop.cancel()
        Ok(())

# Stops a serve whose connection was lost to an allocation failure.
def watchdog(done: CancellationToken, stop: CancellationToken) -> ():
    drop(done.wait_timeout(300u64))
    stop.cancel()

def tag(result: &Result[Report, ServerError]) -> str:
    match result:
        case Ok(_):
            "ok"
        case Err(ServerError.Allocation(_)):
            "alloc"
        case Err(ServerError.Thread(_)):
            "thread"
        case Err(ServerError.Net(_)):
            "net"
        case Err(_):
            "other"

def main() -> Result[(), std.net.NetError | std.http.server.ServerError | ThreadError | AllocError | IoError]:
    mut table = std.http.server.router[Route]()?
    table.add("GET", "/", Route.Home)?
    mut config = std.http.server.config()
    config.workers = 1
    mut streak = 0
    mut attempt = 0
    while streak < 10 and attempt < 3000:
        listener = std.net.listen(&std.net.loopback_v4(0u16), 4)?
        port = listener.local_address()?.port
        stop = CancellationToken()?
        done = CancellationToken()?
        app = Once(stop.share())
        mut client = connect_to(port, "GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")?
        marker = "__test_fail_allocations_after_".concat(str.from(attempt)?)?.concat("__")?
        mut outcome: Result[Report, ServerError] = Err(ServerError.InvalidConfig)
        with spawn(watchdog, done.share(), stop.share())? as task:
            print(marker)?
            outcome = std.http.server.serve(listener, &table, &app, &config, &stop)
            print("__test_restore_allocations__")?
            done.cancel()
        # A connection lost to the failure may end with a reset.
        answer = match until(&mut client, ""):
            case Ok(text):
                text
            case Err(_):
                ""
        served = answer.startswith("HTTP/1.1 200 OK|")
        print(tag(&outcome), served)?
        if served:
            streak += 1
        else:
            streak = 0
        attempt += 1
    Ok(())
"##;
    let text = output(&format!("{LOOPBACK}{main}"));
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines.len() > 20 && lines.len() < 3000, "{text}");
    assert_eq!(lines[0], "alloc False");
    assert!(
        lines[lines.len() - 10..]
            .iter()
            .all(|line| *line == "ok True"),
        "{text}"
    );
    for line in &lines {
        assert!(
            [
                "ok True",
                "ok False",
                "alloc False",
                "thread False",
                "net False"
            ]
            .contains(line),
            "{text}"
        );
    }
}

/// The compiler binary, copied alone into a fresh directory and run with an
/// empty environment apart from `PATH` (for `cc`), builds a JSON service from
/// the embedded standard library with no network or cache; the service
/// answers its client, and links none of TLS, SQLite or zlib.
#[test]
fn a_relocated_compiler_builds_an_offline_json_service() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("toolchain/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let compiler = bin.join("plenty");
    support::install_executable(Path::new(env!("CARGO_BIN_EXE_plenty")), &compiler);
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let source = dir.path().join("service.plenty");
    std::fs::write(
        &source,
        r##"import std.bytes
import std.http
import std.http.body
import std.http.client
import std.http.server
import std.json
import std.net
import std.time
from std.http.server import Exchange
from std.json import Json, Member
from std.net import TcpStream
from std.sync import CancellationToken

pub enum Route:
    Items

# Echoes a JSON object back with a count of its members.
pub class Service:
    pub def handle(self, route: &Route, exchange: &mut Exchange[TcpStream]) -> Result[(), AllocError | std.http.HttpError | std.http.body.BodyError]:
        limits = std.json.limits()
        value = std.http.body.read_json(exchange, &limits)?
        mut size = 0
        match &value:
            case Json.Object(members):
                size = len(members)
            case _:
                exchange.send_text(422, "expected an object")?
                return Ok(())
        mut members = list[Member].new()?
        members.append(Member("members", Json.Int(size)))?
        members.append(Member("echo", value))?
        std.http.body.send_json(exchange, 200, &Json.Object(members), &limits)?
        Ok(())

def visit(port: u16, stop: CancellationToken) -> Result[(), AllocError | IoError | RangeError | std.bytes.BytesError | std.http.client.ClientError]:
    never = CancellationToken()?
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(10u32))?
    mut client = std.http.client.client(std.http.client.config()?)?
    mut post = std.http.client.request("POST", &"http://127.0.0.1:".concat(&str.from(i64(port))?)?.concat("/items")?)?
    post.add("Content-Type", "application/json")?
    post.body = std.bytes.from_text("{\"name\":\"plenty\",\"tags\":[1,2]}")?
    answer = client.fetch(post, &deadline, &never)?
    print(answer.status, answer.body.to_text()?)?
    stop.cancel()
    Ok(())

def main() -> Result[(), AllocError | ThreadError | std.http.server.ServerError | std.net.NetError]:
    mut table = std.http.server.router[Route]()?
    table.add("POST", "/items", Route.Items)?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    with spawn(visit, port, stop.share())? as task:
        drop(std.http.server.serve(listener, &table, &Service(), &std.http.server.config(), &stop)?)
    Ok(())
"##,
    )
    .unwrap();
    let executable = dir.path().join("service");
    let built = Command::new(&compiler)
        .arg("--compile")
        .arg(&source)
        .arg("-o")
        .arg(&executable)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap())
        .env("HOME", &home)
        .env("TMPDIR", dir.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let ran = Command::new(&executable).env_clear().output().unwrap();
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&ran.stdout),
        "200 {\"members\":2,\"echo\":{\"name\":\"plenty\",\"tags\":[1,2]}}\n"
    );
    let backends = backends(&executable);
    assert!(backends.is_empty(), "{backends:?}");
}

/// Symbols of TLS (rustls/ring), SQLite or zlib-rs linked into `executable`.
fn backends(executable: &Path) -> Vec<String> {
    use object::{Object, ObjectSymbol, SymbolKind};
    let data = std::fs::read(executable).unwrap();
    let file = object::File::parse(&*data).unwrap();
    file.symbols()
        .filter(|symbol| symbol.kind() != SymbolKind::File)
        .filter_map(|symbol| symbol.name().ok().map(str::to_owned))
        .filter(|name| {
            name == "plenty_tls"
                || name.contains("ring_core_")
                || name == "plenty_sqlite"
                || name.starts_with("sqlite3")
                || name == "plenty_compress"
                || name.contains("zlib_rs")
        })
        .collect()
}

/// The dependency of the installed package: a CSV summary with typed errors.
const STOCK: &str = r##"
import std.csv
import std.io
from std.csv import CsvError
from std.io import StreamError
from std.json import Json, Member

pub enum SummaryError:
    Open(StreamError)
    Csv(CsvError)
    Count(i64, ParseError)
    Allocation(AllocError)

def opened[T](result: Result[T, StreamError]) -> Result[T, SummaryError]:
    match result:
        case Ok(value):
            Ok(value)
        case Err(cause):
            Err(SummaryError.Open(cause))

def csv[T](result: Result[T, CsvError]) -> Result[T, SummaryError]:
    match result:
        case Ok(value):
            Ok(value)
        case Err(cause):
            Err(SummaryError.Csv(cause))

def grown[T](result: Result[T, AllocError]) -> Result[T, SummaryError]:
    match result:
        case Ok(value):
            Ok(value)
        case Err(cause):
            Err(SummaryError.Allocation(cause))

# Counts the records of a CSV file after its header and totals integer column
# `column`, which the header names.
pub def summarize(path: &str, column: i64) -> Result[Json, SummaryError]:
    mut input = csv(std.csv.reader(opened(std.io.open_file(path, "r"))?, &std.csv.rfc4180(), 4096))?
    mut record = csv(std.csv.record())?
    mut name = ""
    if csv(input.read(&mut record))?:
        name = csv(record.text(column))?
    mut rows = 0
    mut total = 0
    while csv(input.read(&mut record))?:
        match i64.parse(&csv(record.text(column))?):
            case Ok(count):
                total += count
            case Err(cause):
                return Err(SummaryError.Count(record.number(), cause))
        rows += 1
    members = grown([
        Member("column", Json.Text(name)),
        Member("rows", Json.Int(rows)),
        Member("total", Json.Int(total))
    ])?
    Ok(Json.Object(members))
"##;

/// A CLI/data tool: `tally --column N FILE` prints a JSON summary of a CSV file.
const TALLY: &str = r##"
import std.cli
import std.json
import std.path
import stock
from std.cli import Spec, Kind, Value, parse, help, describe_for, exit_status
from std.error import Context, Subject

# tally --column N FILE: prints a JSON summary of one integer CSV column.
def run() -> Result[i32, AllocError | IoError | std.cli.CliError | std.json.JsonError]:
    schema = [
        Spec("--column", Kind.Integer, False, True, False, "column to total"),
        Spec("FILE", Kind.Text, True, True, False, "CSV file with a header")
    ]?
    raw = args_bytes()?
    match parse(&schema, &raw, 1):
        case Err(error):
            status = exit_status(&error)
            context = Context("arguments", Subject.Absent, error)
            context.report_with(describe_for(&context.cause, &schema))
            Ok(status)
        case Ok(parsed):
            if parsed.help_requested:
                drop(write_stdout(help(&schema, "tally")?))
                return Ok(0i32)
            mut column = 0
            mut file = ""
            for entry in &parsed.entries:
                match &entry.value:
                    case Value.Integer(value):
                        column = *value
                    case Value.Text(value):
                        file = "".concat(value)?
                    case _:
                        pass
            match stock.summarize(&file, column):
                case Ok(summary):
                    print(std.json.encode_text(&summary, &std.json.limits())?)?
                    Ok(0i32)
                case Err(cause):
                    Context("summarize", Subject.At(std.path.from_text(&file)?), cause).report()
                    Ok(1i32)

def main() -> i32:
    match run():
        case Ok(status):
            status
        case Err(error):
            Context("tally", Subject.Absent, error).report()
            1i32
"##;

/// A JSON service answering `GET /summary` for the CSV file it is given, with
/// a client in the same process that fetches it once and stops the server.
const SERVICE: &str = r##"
import std.bytes
import std.http.body
import std.http.client
import std.http.server
import std.json
import std.net
import std.time
import stock
from std.http.server import Exchange
from std.net import TcpStream
from std.sync import CancellationToken

pub enum Route:
    Summary

# Answers GET /summary with the JSON summary of a CSV file.
pub class Service:
    pub path: str

    pub def handle(self, route: &Route, exchange: &mut Exchange[TcpStream]) -> Result[(), stock.SummaryError | std.http.body.BodyError]:
        summary = stock.summarize(&self.path, 1)?
        std.http.body.send_json(exchange, 200, &summary, &std.json.limits())?
        Ok(())

def visit(port: u16, stop: CancellationToken) -> Result[(), AllocError | IoError | RangeError | std.bytes.BytesError | std.http.client.ClientError]:
    never = CancellationToken()?
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(10u32))?
    mut client = std.http.client.client(std.http.client.config()?)?
    get = std.http.client.request("GET", &"http://127.0.0.1:".concat(&str.from(i64(port))?)?.concat("/summary")?)?
    answer = client.fetch(get, &deadline, &never)?
    print(answer.status, answer.body.to_text()?)?
    stop.cancel()
    Ok(())

def main() -> Result[(), AllocError | IoError | ThreadError | std.http.server.ServerError | std.net.NetError]:
    arguments = args()?
    mut table = std.http.server.router[Route]()?
    table.add("GET", "/summary", Route.Summary)?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    with spawn(visit, port, stop.share())? as task:
        drop(std.http.server.serve(listener, &table, &Service("".concat(&arguments[1])?), &std.http.server.config(), &stop)?)
    Ok(())
"##;

/// Run git in `directory` with no user or system configuration.
fn git(directory: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args(["-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
        .args(["-c", "init.defaultBranch=main"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .current_dir(directory)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The compiler installed alone as `PREFIX/bin/plenty`, run as a user would
/// run it in a new account: an empty environment apart from `PATH` (for `cc`
/// and `git`), a fresh home and so a cold compiled-function cache. A package
/// with a Git dependency is fetched once; the dependency's repository is then
/// deleted and `git` replaced by a failing stub, and `plenty build --offline
/// --locked` builds a CLI/data tool and a JSON service from the package cache
/// and the embedded standard library. Both run, the tool's usage and data
/// errors have their statuses, and neither links TLS, SQLite or zlib.
#[test]
fn an_installed_toolchain_builds_a_cli_tool_and_a_service_offline() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("prefix/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let compiler = bin.join("plenty");
    support::install_executable(Path::new(env!("CARGO_BIN_EXE_plenty")), &compiler);
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let write = |path: &Path, text: &str| {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };

    let repo = dir.path().join("stock");
    write(
        &repo.join("plenty.toml"),
        "manifest-version = 1\n[package]\nname = \"stock\"\nversion = \"1.0.0\"\n",
    );
    write(&repo.join("src/lib.plenty"), STOCK);
    git(&repo, &["init", "--quiet"]);
    git(&repo, &["add", "--all"]);
    git(&repo, &["commit", "--quiet", "--message", "stock"]);
    git(&repo, &["tag", "v1"]);

    let app = dir.path().join("inventory");
    write(
        &app.join("plenty.toml"),
        &format!(
            "manifest-version = 1\n[package]\nname = \"inventory\"\nversion = \"0.1.0\"\n\n\
             [[bin]]\nname = \"tally\"\npath = \"src/tally.plenty\"\n\n\
             [[bin]]\nname = \"service\"\npath = \"src/service.plenty\"\n\n\
             [dependencies]\nstock = {{ git = \"file://{}\", tag = \"v1\" }}\n",
            repo.display()
        ),
    );
    write(&app.join("src/tally.plenty"), TALLY);
    write(&app.join("src/service.plenty"), SERVICE);
    let data = dir.path().join("stock.csv");
    std::fs::write(&data, "item,count\r\npen,3\r\nbook,12\r\ncard,2\r\n").unwrap();
    let bad = dir.path().join("bad.csv");
    std::fs::write(&bad, "item,count\npen,3\nbook,many\n").unwrap();

    let plenty = |path: &std::ffi::OsStr, args: &[&str]| {
        let output = Command::new(&compiler)
            .args(args)
            .env_clear()
            .env("PATH", path)
            .env("HOME", &home)
            .env("TMPDIR", dir.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .current_dir(&app)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "plenty {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    };
    let path = std::env::var_os("PATH").unwrap();
    plenty(&path, &["fetch"]);
    assert!(app.join("plenty.lock").is_file());
    std::fs::remove_dir_all(&repo).unwrap();

    let stubs = dir.path().join("stubs");
    std::fs::create_dir_all(&stubs).unwrap();
    support::write_script(
        &stubs.join("git"),
        "#!/bin/sh\necho 'git must not run offline' >&2\nexit 1\n",
    );
    let mut offline = std::ffi::OsString::from(&stubs);
    offline.push(":");
    offline.push(&path);
    plenty(&offline, &["build", "--offline", "--locked"]);
    let ran = plenty(
        &offline,
        &[
            "run",
            "--offline",
            "--locked",
            "--bin",
            "tally",
            "--",
            "--column",
            "1",
            data.to_str().unwrap(),
        ],
    );
    let summary = "{\"column\":\"count\",\"rows\":3,\"total\":17}";
    assert_eq!(String::from_utf8_lossy(&ran.stdout), format!("{summary}\n"));

    let built = app.join("target/debug");
    let tally = |args: &[&std::ffi::OsStr]| {
        Command::new(built.join("tally"))
            .args(args)
            .env_clear()
            .output()
            .unwrap()
    };
    let help = tally(&["--help".as_ref()]);
    assert_eq!(help.status.code(), Some(0), "{help:?}");
    assert!(
        help.stdout.starts_with(b"Usage: tally [--help]\n"),
        "{help:?}"
    );
    let usage = tally(&[data.as_os_str()]);
    assert_eq!(usage.status.code(), Some(2), "{usage:?}");
    assert_eq!(
        String::from_utf8_lossy(&usage.stderr),
        "error: missing required argument --column\n"
    );
    let invalid = tally(&["--column".as_ref(), "1".as_ref(), bad.as_os_str()]);
    assert_eq!(invalid.status.code(), Some(1), "{invalid:?}");
    assert_eq!(
        String::from_utf8_lossy(&invalid.stderr),
        format!(
            "error: summarize {:?}: stock::lib.SummaryError.Count(3, ParseError.Invalid)\n",
            bad.display().to_string()
        )
    );
    let missing = dir.path().join("missing.csv");
    let absent = tally(&["--column".as_ref(), "1".as_ref(), missing.as_os_str()]);
    assert_eq!(absent.status.code(), Some(1), "{absent:?}");
    assert!(
        String::from_utf8_lossy(&absent.stderr).contains("SummaryError.Open("),
        "{absent:?}"
    );

    let served = Command::new(built.join("service"))
        .arg(&data)
        .env_clear()
        .output()
        .unwrap();
    assert!(served.status.success(), "{served:?}");
    assert_eq!(
        String::from_utf8_lossy(&served.stdout),
        format!("200 {summary}\n")
    );
    for name in ["tally", "service"] {
        let backends = backends(&built.join(name));
        assert!(backends.is_empty(), "{name}: {backends:?}");
    }
}
