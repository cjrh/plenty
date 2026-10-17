//! Production qualification suites for `std.http.server` (issue #103, B66):
//! a retained request corpus and its mutations, interoperability with curl
//! and `std.http.client`, slow clients, shutdown under sustained load,
//! descriptor and heap leaks, allocation failure on the accepting thread, and
//! a relocated compiler building a JSON service. Results and the gates they
//! cover are in `book/src/design/stdlib/http-qualification.md`. Everything
//! runs on loopback; no test needs a network.

use std::io::{BufRead, BufReader, Read};
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

# Routes: 0 GET / -> "home"; 1 POST /echo; 2 GET /big -> 1 MiB chunked;
# 3 GET /huge -> 64 MiB with Content-Length; 4 GET /stop cancels `stop`;
# 5 GET /stream -> two chunks.
pub class App:
    pub stop: CancellationToken
    pub piece: Bytes

    pub def handle(self, exchange: &mut Exchange[TcpStream]) -> Result[(), Failure]:
        route = exchange.route
        if route == 0:
            exchange.send_text(200, "home")?
        elif route == 1:
            mut body = std.bytes.empty()?
            exchange.read_all(&mut body)?
            exchange.send(200, "application/octet-stream", &body)?
        elif route == 2:
            exchange.start(std.http.response(200, "OK")?)?
            for index in range(16):
                exchange.write(&self.piece, 0, self.piece.len())?
        elif route == 3:
            mut head = std.http.response(200, "OK")?
            head.add("Content-Length", str.from(1024 * self.piece.len())?)?
            exchange.start(head)?
            for index in range(1024):
                exchange.write(&self.piece, 0, self.piece.len())?
            exchange.end()?
        elif route == 4:
            exchange.send_text(200, "stopping")?
            self.stop.cancel()
        else:
            exchange.start(std.http.response(200, "OK")?)?
            text = std.bytes.from_text("abcdef")?
            exchange.write(&text, 0, 2)?
            exchange.write(&text, 2, 6)?
        Ok(())

pub def app(stop: &CancellationToken) -> Result[App, Failure]:
    mut piece = std.bytes.with_capacity(65536)?
    piece.resize(65536, 120u8)?
    Ok(App(stop.share(), piece))

pub def routes() -> Result[Router, ServerError]:
    mut table = std.http.server.router()?
    table.add("GET", "/", 0)?
    table.add("POST", "/echo", 1)?
    table.add("GET", "/big", 2)?
    table.add("GET", "/huge", 3)?
    table.add("GET", "/stop", 4)?
    table.add("GET", "/stream", 5)?
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
pub def descriptors() -> Result[i64, Failure]:
    mut count = 0
    for entry in std.fs.entries(std.fs.read_dir(&std.path.from_text("/proc/self/fd")?)?):
        entry?
        count += 1
    Ok(count)

pub def counts(report: &Report) -> Result[str, Failure]:
    mut text = "connections=".concat(str.from(report.connections)?)?
    text = text.concat(" requests=")?.concat(str.from(report.requests)?)?
    text = text.concat(" handler_errors=")?.concat(str.from(report.handler_errors)?)?
    text = text.concat(" protocol_errors=")?.concat(str.from(report.protocol_errors)?)?
    text = text.concat(" timeouts=")?.concat(str.from(report.timeouts)?)?
    text = text.concat(" cancelled=")?.concat(str.from(report.cancelled)?)?
    text = text.concat(" rejected=")?.concat(str.from(report.rejected)?)?
    Ok(text)
"##;

fn build(source: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    (dir, executable)
}

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
        format!("{SCRIPTED}\ndef main() -> Result[(), Failure]:\n    replay(\"{blob}\", {step})?\n    Ok(())\n");
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
def main() -> Result[(), Failure]:
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

    pub def handle(self, exchange: &mut Exchange[TlsTransport]) -> Result[(), Failure]:
        if exchange.route == 1:
            mut body = std.bytes.empty()?
            exchange.read_all(&mut body)?
            exchange.send(200, "application/octet-stream", &body)?
        elif exchange.route == 4:
            exchange.send_text(200, "stopping")?
            self.stop.cancel()
        else:
            exchange.send_text(200, "secure")?
        Ok(())

def main() -> Result[(), Failure]:
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
    // The refused client leaves during its handshake, which is not counted
    // as a protocol error.
    assert_eq!(
        server.finish(),
        "connections=4 requests=4 handler_errors=0 protocol_errors=0 timeouts=0 cancelled=0 rejected=0"
    );
}

/// `std.http.client` against the server: pooled keep-alive requests, a
/// streamed chunked upload, sized and chunked downloads, and `HEAD`.
#[test]
fn std_http_client_interoperates() {
    let main = r##"import std.http.client
import std.io

def visit(port: u16, stop: CancellationToken) -> Result[(), Failure]:
    never = CancellationToken()?
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(20u32))?
    base = "http://127.0.0.1:".concat(&str.from(i64(port))?)?
    mut client = std.http.client.client(std.http.client.config()?)?
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

def main() -> Result[(), Failure]:
    table = routes()?
    stop = CancellationToken()?
    site = app(&stop)?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 64)?
    port = listener.local_address()?.port
    with spawn(visit, port, stop.share())? as task:
        report = std.http.server.serve(listener, &table, &site, &std.http.server.config(), &stop)?
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

def main() -> Result[(), Failure]:
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

def main() -> Result[(), Failure]:
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
    pub def handle(self, exchange: &mut Exchange[TlsTransport]) -> Result[(), Failure]:
        exchange.send_text(200, "secure")?
        Ok(())

def visit(port: u16, stop: CancellationToken) -> Result[(), Failure]:
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

def main() -> Result[(), Failure]:
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

def load_round() -> Result[(), Failure]:
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

def fault_round() -> Result[(), Failure]:
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
        # Whether a partial head is still parked when the silent clients
        # arrive decides how many of the 64 connections are turned away.
        print(report.connections + report.rejected, report.requests, report.protocol_errors, report.handler_errors, report.rejected >= 2u64)?
    Ok(())

# Each round's values are dropped when it returns, so the heap checkpoint
# after it sees only what the round leaked.
def main() -> Result[(), Failure]:
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
    assert_eq!(lines[2..].join("\n"), "0\n64 20 40 0 True\n0", "{text}");
}

/// A handler's `Err` is answered with `500` and counted, and the server goes
/// on; a fatal trap in a handler (here an index out of range) ends the whole
/// process at once with a located message, cutting off every connection.
#[test]
fn a_handler_error_is_answered_but_a_trap_ends_the_process() {
    let main = r##"
pub class Fragile:
    pub def handle(self, exchange: &mut Exchange[TcpStream]) -> Result[(), Failure]:
        if exchange.route == 0:
            return Err(Failure.Unspecified)
        values = [1, 2, 3]?
        exchange.send_text(200, &str.from(values[exchange.route + 5])?)?
        Ok(())

def client(port: u16, stop: CancellationToken) -> ():
    print(fetch(port, "GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap()).unwrap()
    drop(fetch(port, "GET /trap HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"))
    print("unreachable").unwrap()

def main() -> Result[(), Failure]:
    mut table = std.http.server.router()?
    table.add("GET", "/", 0)?
    table.add("GET", "/trap", 1)?
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

    pub def handle(self, exchange: &mut Exchange[TcpStream]) -> Result[(), Failure]:
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

def main() -> Result[(), Failure]:
    mut table = std.http.server.router()?
    table.add("GET", "/", 0)?
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
    use object::{Object, ObjectSymbol, SymbolKind};
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("toolchain/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let compiler = bin.join("plenty");
    std::fs::copy(env!("CARGO_BIN_EXE_plenty"), &compiler).unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let source = dir.path().join("service.plenty");
    std::fs::write(
        &source,
        r##"import std.bytes
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

# Echoes a JSON object back with a count of its members.
pub class Service:
    pub def handle(self, exchange: &mut Exchange[TcpStream]) -> Result[(), Failure]:
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

def visit(port: u16, stop: CancellationToken) -> Result[(), Failure]:
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

def main() -> Result[(), Failure]:
    mut table = std.http.server.router()?
    table.add("POST", "/items", 0)?
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
    let data = std::fs::read(&executable).unwrap();
    let file = object::File::parse(&*data).unwrap();
    let backends: Vec<String> = file
        .symbols()
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
        .collect();
    assert!(backends.is_empty(), "{backends:?}");
}
