//! `std.http.files` and signed cookies in `std.http.cookie`. Requests run in
//! memory through `serve_connection`; nothing needs a network. The multipart
//! allocation tests run under `plenty test`, whose builds inject failures.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime};

/// An in-memory `Transport` that echoes responses, and a handler serving
/// `$SITE` below `/s/` and reading a signed `session` cookie on `/session`.
const PRELUDE: &str = r##"import std.bytes
import std.fs
import std.http
import std.http.cookie
import std.http.files
import std.http.server
import std.path
from std.bytes import Bytes
from std.fs import Root
from std.http.cookie import CookieError
from std.http.server import Exchange, Router
from std.net import NetError, Shutdown
from std.sync import CancellationToken
from std.time import Deadline

pub class Script:
    pub input: Bytes
    pub at: i64

    pub def read_before(self: &mut Script, buffer: &mut Bytes, start: i64, stop: i64, deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, NetError]:
        left = self.input.len() - self.at
        if left == 0:
            return Ok(0)
        mut count = stop - start
        if count > left:
            count = left
        buffer.overwrite(start, &self.input, self.at, self.at + count).unwrap()
        self.at += count
        Ok(count)

    pub def shutdown(self: &mut Script, direction: Shutdown) -> Result[(), NetError]:
        Ok(())

    pub def write_before(self: &mut Script, source: &Bytes, start: i64, stop: i64, deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, NetError]:
        Ok(source.write_stdout(start, stop).unwrap())

pub class App:
    pub site: Root
    pub keys: list[Bytes]

    pub def handle(self, exchange: &mut Exchange[Script]) -> Result[(), Failure]:
        if exchange.route == 0:
            path = exchange.params[0].concat("")?
            std.http.files.serve(exchange, &self.site, &path)?
        else:
            found = std.http.cookie.cookies(&exchange.request.headers, 8)?
            match std.http.cookie.find_signed(&found, &self.keys, "session")?:
                case Some(value):
                    exchange.send_text(200, &value)?
                case Nothing:
                    exchange.send_text(401, "unsigned")?
        Ok(())

pub def key(fill: u8) -> Result[Bytes, Failure]:
    mut data = std.bytes.empty()?
    data.resize(32, fill)?
    Ok(data)

pub def app(keys: list[Bytes]) -> Result[App, Failure]:
    Ok(App(std.fs.root(&std.path.from_text("$SITE")?)?, keys))

pub def routes() -> Result[Router, Failure]:
    mut table = std.http.server.router()?
    table.add("*", "/s/{path...}", 0)?
    table.add("GET", "/session", 1)?
    Ok(table)

# Serves one request line plus `fields` and prints the response.
pub def ask(app: &App, line: &str, fields: &str) -> Result[(), Failure]:
    table = routes()?
    stop = CancellationToken()?
    cancel = CancellationToken()?
    config = std.http.server.config()
    text = line.concat(" HTTP/1.1\r\nHost: x\r\nConnection: close\r\n")?.concat(fields)?.concat("\r\n")?
    report = std.http.server.serve_connection(Script(std.bytes.from_text(&text)?, 0), &table, app, &config, &stop, &cancel)
    print("")?
    Ok(())
"##;

/// 2001-09-09T01:46:40.5Z: second 1_000_000_000, half a second in.
const MTIME: u64 = 1_000_000_000;
const TAG: &str = "W/\"12-1000000000.500000000\"";
const DATE: &str = "Sun, 09 Sep 2001 01:46:40 GMT";

/// A site directory with a page, a stylesheet, a hidden file, links that
/// escape it, and a secret beside it. Every file has the same mtime.
fn site(dir: &Path) -> std::path::PathBuf {
    let site = dir.join("site");
    std::fs::create_dir_all(site.join("css")).unwrap();
    std::fs::create_dir_all(site.join(".git")).unwrap();
    let files = [
        (site.join("index.html"), "<h1>hi</h1>\n".to_string()),
        (site.join("css/a.css"), "body{}\n".to_string()),
        (site.join(".git/config"), "secret\n".to_string()),
        (site.join("big.bin"), "0123456789".repeat(5000)),
        (dir.join("secret.txt"), "secret\n".to_string()),
    ];
    for (path, text) in &files {
        std::fs::write(path, text).unwrap();
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(SystemTime::UNIX_EPOCH + Duration::from_millis(MTIME * 1000 + 500))
            .unwrap();
    }
    std::os::unix::fs::symlink("../secret.txt", site.join("link.txt")).unwrap();
    std::os::unix::fs::symlink("..", site.join("up")).unwrap();
    std::os::unix::fs::symlink("index.html", site.join("alias.html")).unwrap();
    let fifo = site.join("pipe");
    assert!(Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());
    site
}

/// Compiles and runs `main` after the prelude, with responses joined by `|`.
fn run(main: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let site = site(dir.path());
    let source = format!("{PRELUDE}{main}").replace("$SITE", site.to_str().unwrap());
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, &source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{main}\n{error}"));
    let output = Command::new(executable)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{main}\nstatus {:?}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .replace("\r\n", "|")
        .replace("Connection: close|", "")
}

#[test]
fn files_are_served_with_type_and_validators() {
    let output = run(r#"
def main() -> Result[(), Failure]:
    app = app([]?)?
    ask(&app, "GET /s/index.html", "")?
    ask(&app, "GET /s/css/a.css", "")?
    ask(&app, "HEAD /s/index.html", "")?
    ask(&app, "GET /s/alias.html", "")?
    ask(&app, "POST /s/index.html", "Content-Length: 0\r\n")?
    Ok(())
"#);
    let page = format!(
        "HTTP/1.1 200 OK|Content-Type: text/html; charset=utf-8|Content-Length: 12|X-Content-Type-Options: nosniff|ETag: {TAG}|Last-Modified: {DATE}||"
    );
    let expected = [
        format!("{page}<h1>hi</h1>\n\n"),
        format!(
            "HTTP/1.1 200 OK|Content-Type: text/css; charset=utf-8|Content-Length: 7|X-Content-Type-Options: nosniff|ETag: W/\"7-1000000000.500000000\"|Last-Modified: {DATE}||body{{}}\n\n"
        ),
        format!("{page}\n"),
        "HTTP/1.1 404 Not Found|Content-Type: text/plain; charset=utf-8|Content-Length: 10||Not Found\n\n".to_string(),
        "HTTP/1.1 405 Method Not Allowed|Allow: GET, HEAD|Content-Type: text/plain; charset=utf-8|Content-Length: 19||Method Not Allowed\n\n".to_string(),
    ];
    assert_eq!(output, expected.concat());
}

/// Every refused path looks like a missing file, so nothing about what lies
/// outside the root, or about hidden entries, is revealed.
#[test]
fn paths_outside_the_root_are_not_found() {
    let output = run(r#"
def main() -> Result[(), Failure]:
    app = app([]?)?
    for target in ["/s/link.txt", "/s/up/secret.txt", "/s/..%2fsecret.txt", "/s/%2e%2e/secret.txt", "/s/css/..%2f..%2fsecret.txt", "/s/%2fetc%2fpasswd", "/s//etc/passwd", "/s/a%00b", "/s/.git/config", "/s/css", "/s/css/", "/s/pipe", "/s/missing", "/s/css%2f.%2fa.css"]?:
        ask(&app, &"GET ".concat(target)?, "")?
    Ok(())
"#);
    let missing = "HTTP/1.1 404 Not Found|Content-Type: text/plain; charset=utf-8|Content-Length: 10||Not Found\n\n";
    assert_eq!(output, missing.repeat(14));
}

#[test]
fn conditional_requests_answer_not_modified() {
    let output = run(&format!(
        r#"
def main() -> Result[(), Failure]:
    app = app([]?)?
    ask(&app, "GET /s/index.html", "If-None-Match: {tag}\r\n")?
    ask(&app, "GET /s/index.html", "If-None-Match: \"other\", {strong}\r\n")?
    ask(&app, "HEAD /s/index.html", "If-None-Match: *\r\n")?
    ask(&app, "GET /s/index.html", "If-Modified-Since: {date}\r\n")?
    ask(&app, "GET /s/index.html", "If-Modified-Since: Sun, 09 Sep 2001 01:46:39 GMT\r\n")?
    ask(&app, "GET /s/index.html", "If-None-Match: \"other\"\r\nIf-Modified-Since: {date}\r\n")?
    ask(&app, "GET /s/index.html", "If-Modified-Since: yesterday\r\n")?
    Ok(())
"#,
        tag = TAG.replace('"', "\\\""),
        strong = TAG[2..].replace('"', "\\\""),
        date = DATE,
    ));
    let unchanged = format!("HTTP/1.1 304 Not Modified|ETag: {TAG}|Last-Modified: {DATE}||\n");
    let page = format!(
        "HTTP/1.1 200 OK|Content-Type: text/html; charset=utf-8|Content-Length: 12|X-Content-Type-Options: nosniff|ETag: {TAG}|Last-Modified: {DATE}||<h1>hi</h1>\n\n"
    );
    let expected = [
        unchanged.clone(),
        unchanged.clone(),
        unchanged.clone(),
        unchanged,
        page.clone(),
        page.clone(),
        page,
    ];
    assert_eq!(output, expected.concat());
}

/// A file larger than the 16 KiB buffer arrives whole.
#[test]
fn large_files_stream_in_chunks() {
    let output = run(r#"
def main() -> Result[(), Failure]:
    app = app([]?)?
    ask(&app, "GET /s/big.bin", "")?
    Ok(())
"#);
    let (head, body) = output.split_once("||").unwrap();
    assert!(head.contains("Content-Type: application/octet-stream|Content-Length: 50000|"));
    assert_eq!(body, format!("{}\n", "0123456789".repeat(5000)));
}

#[test]
fn media_types_and_http_dates() {
    let output = run(r#"
def main() -> Result[(), Failure]:
    for name in ["a.HTML", "b.js", "c.json", "d.svg", "e.png", "f.jpeg", "g.wasm", "h.woff2", "i.tar.gz", "noext", ".css"]?:
        print(std.http.files.media_type(name))?
    for seconds in [0, 784111777, 951782400, 253402300799, 253402300800, -1]?:
        print(std.http.files.http_date(seconds)?)?
    for text in ["Sun, 06 Nov 1994 08:49:37 GMT", "Tue, 29 Feb 2000 00:00:00 GMT", "Sunday, 06-Nov-94 08:49:37 GMT", "Sun, 06 Nov 1994 08:49:37 UTC", "Sun, 31 Feb 1994 08:49:37 GMT", "Sun, 06 Nov 1994 24:00:00 GMT", ""]?:
        print(std.http.files.parse_http_date(text)?)?
    Ok(())
"#);
    let expected = "\
text/html; charset=utf-8
text/javascript; charset=utf-8
application/json
image/svg+xml
image/png
image/jpeg
application/wasm
font/woff2
application/octet-stream
application/octet-stream
text/css; charset=utf-8
Option[str].Some(\"Thu, 01 Jan 1970 00:00:00 GMT\")
Option[str].Some(\"Sun, 06 Nov 1994 08:49:37 GMT\")
Option[str].Some(\"Tue, 29 Feb 2000 00:00:00 GMT\")
Option[str].Some(\"Fri, 31 Dec 9999 23:59:59 GMT\")
Option[str].Nothing
Option[str].Nothing
Option[i64].Some(784111777)
Option[i64].Some(951782400)
Option[i64].Nothing
Option[i64].Nothing
Option[i64].Nothing
Option[i64].Nothing
Option[i64].Nothing
";
    assert_eq!(output, expected);
}

/// `file_metadata` observes the open file, so removing its path afterwards
/// and putting a directory there does not change the answer.
#[test]
fn file_metadata_observes_the_open_file() {
    let output = run(r#"
def main() -> Result[(), Failure]:
    site = std.fs.root(&std.path.from_text("$SITE")?)?
    stream = site.read_file(&std.path.from_text("index.html")?)?
    site.remove_file(&std.path.from_text("index.html")?)?
    site.create_dir(&std.path.from_text("index.html")?)?
    found = std.fs.file_metadata(&stream)?
    print(found.size())?
    print(found.is_file())?
    print(found.modified())?
    Ok(())
"#);
    assert_eq!(
        output,
        "12\nTrue\nOption[std.time.Timestamp].Some(std.time.Timestamp(seconds=1000000000, nanoseconds=500000000))\n"
    );
}

#[test]
fn signed_values_verify_under_any_listed_key() {
    let output = run(r#"
def main() -> Result[(), Failure]:
    old = [key(1u8)?]?
    rotated = [key(2u8)?, key(1u8)?]?
    signed = std.http.cookie.sign(&old, "session", "user42")?
    print(signed)?
    print(std.http.cookie.verify(&old, "session", &signed)?)?
    print(std.http.cookie.verify(&rotated, "session", &signed)?)?
    print(std.http.cookie.verify(&[key(2u8)?]?, "session", &signed)?)?
    print(std.http.cookie.verify(&old, "other", &signed)?)?
    print(std.http.cookie.verify(&old, "session", &"user43".concat(signed.slice(6, signed.byte_len())?)?)?)?
    print(std.http.cookie.verify(&old, "session", "user42")?)?
    print(std.http.cookie.verify(&old, "session", "user42.!!")?)?
    print(std.http.cookie.sign(&rotated, "session", "user42")? == signed)?
    print(std.http.cookie.signed_cookie(&rotated, "session", "")?.format()?)?
    short = [std.bytes.from_text("too short")?]?
    for result in [std.http.cookie.sign(&short, "session", "x"), std.http.cookie.sign(&[]?, "session", "x"), std.http.cookie.sign(&old, "bad name", "x"), std.http.cookie.sign(&old, "session", "a;b")]?:
        match result:
            case Err(CookieError.InvalidKey):
                print("InvalidKey")?
            case Err(_):
                print("refused")?
            case Ok(_):
                print("signed")?
    Ok(())
"#);
    let expected = "\
user42.FIRST
Option[str].Some(\"user42\")
Option[str].Some(\"user42\")
Option[str].Nothing
Option[str].Nothing
Option[str].Nothing
Option[str].Nothing
Option[str].Nothing
False
session=.TAG; Path=/; Secure; HttpOnly; SameSite=Lax
InvalidKey
InvalidKey
refused
refused
";
    let expected = expected
        .replace("FIRST", &signature(&[1; 32], b"session=user42"))
        .replace("TAG", &signature(&[2; 32], b"session="));
    assert_eq!(output, expected);
}

#[test]
fn signed_cookies_are_found_in_requests() {
    let tag = signature(&[1; 32], b"session=user42");
    let output = run(&format!(
        r#"
def main() -> Result[(), Failure]:
    app = app([key(2u8)?, key(1u8)?]?)?
    ask(&app, "GET /session", "Cookie: theme=dark; session=user42.{tag}\r\n")?
    ask(&app, "GET /session", "Cookie: session=user43.{tag}\r\n")?
    ask(&app, "GET /session", "Cookie: session=user42\r\n")?
    ask(&app, "GET /session", "")?
    Ok(())
"#
    ));
    let unsigned = "HTTP/1.1 401 Unauthorized|Content-Type: text/plain; charset=utf-8|Content-Length: 8||unsigned\n";
    assert_eq!(
        output,
        format!(
            "HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 6||user42\n{unsigned}{unsigned}{unsigned}"
        )
    );
}

/// HMAC-SHA256 of `message` under `key`, unpadded base64url, computed with
/// `openssl` as an independent reference.
fn signature(key: &[u8], message: &[u8]) -> String {
    use std::io::Write;
    let hex: String = key.iter().map(|byte| format!("{byte:02x}")).collect();
    let mut child = Command::new("openssl")
        .args(["dgst", "-sha256", "-mac", "HMAC", "-macopt"])
        .arg(format!("hexkey:{hex}"))
        .arg("-binary")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(message).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    base64url(&output.stdout)
}

fn base64url(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut text = String::new();
    for chunk in data.chunks(3) {
        let value = chunk.iter().enumerate().fold(0u32, |acc, (index, byte)| {
            acc | (u32::from(*byte) << (16 - 8 * index))
        });
        for index in 0..=chunk.len() {
            text.push(ALPHABET[(value >> (18 - 6 * index) & 63) as usize] as char);
        }
    }
    text
}

/// Runs under `plenty test`, whose builds inject failures. The budget rises
/// one allocation (or one file call) at a time until the upload parses; every
/// shorter budget must fail as an allocation or storage error, never as a
/// malformed body, and must leave no descriptor open.
const MULTIPART: &str = r##"import std.bytes
import std.fs
import std.http.body
import std.http.server
import std.path
import std.test
from std.bytes import Bytes
from std.fs import Root
from std.http.body import BodyError, Content, MultipartLimits
from std.http.server import Exchange
from std.io import StreamError
from std.net import NetError, Shutdown
from std.sync import CancellationToken
from std.time import Deadline

pub class Script:
    pub input: Bytes
    pub at: i64

    pub def read_before(self: &mut Script, buffer: &mut Bytes, start: i64, stop: i64, deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, NetError]:
        left = self.input.len() - self.at
        if left == 0:
            return Ok(0)
        mut count = stop - start
        if count > 7:
            count = 7
        if count > left:
            count = left
        buffer.overwrite(start, &self.input, self.at, self.at + count).unwrap()
        self.at += count
        Ok(count)

    pub def shutdown(self: &mut Script, direction: Shutdown) -> Result[(), NetError]:
        Ok(())

    pub def write_before(self: &mut Script, source: &Bytes, start: i64, stop: i64, deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, NetError]:
        Ok(stop - start)

# Each outcome cancels one token, so the test sees it after the connection.
pub class Upload:
    pub storage: Root
    pub limits: MultipartLimits
    pub budget: u32
    pub files: bool
    pub parsed: CancellationToken
    pub refused: CancellationToken
    pub wrong: CancellationToken

    pub def handle(self, exchange: &mut Exchange[Script]) -> Result[(), Failure]:
        if self.files:
            std.test.fail_files_after(self.budget, 28i32)?
        else:
            std.test.fail_allocations_after(self.budget)?
        result = std.http.body.read_multipart(exchange, &self.limits, &self.storage)
        std.test.restore_allocations()?
        std.test.restore_files()?
        match result:
            case Ok(found):
                mut parts = found
                std.test.equal(&len(parts), &2)?
                std.test.equal(&parts[0].name, &"title")?
                std.test.equal(&parts[1].size, &11)?
                match &mut parts[1].content:
                    case Content.File(file):
                        mut data = std.bytes.empty()?
                        data.resize(16, 0u8)?
                        std.test.equal(&file.stream.read(&mut data, 0, 16)?, &11)?
                    case Content.Text(_):
                        std.test.fail("the file part was kept in memory")?
                self.parsed.cancel()
            case Err(BodyError.Allocation(_)):
                self.refused.cancel()
            case Err(BodyError.Storage(StreamError.Io(IoError.System(28i32)))):
                self.refused.cancel()
            case Err(other):
                print(other)?
                self.wrong.cancel()
        exchange.send_text(200, "done")?
        Ok(())

def open_files() -> Result[i64, Failure]:
    mut names = std.fs.root(&std.path.from_text("/proc/self/fd")?)?.names()?
    mut count = 0
    while names.next()?.is_some():
        count += 1
    Ok(count)

def request() -> Result[Bytes, Failure]:
    body = "--XyZ\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nhello\r\n--XyZ\r\nContent-Disposition: form-data; name=\"doc\"; filename=\"a.txt\"\r\nContent-Type: text/csv\r\n\r\nx,y\r\n1,2\r\n3\r\n--XyZ--"
    mut text = "POST /upload HTTP/1.1\r\nHost: x\r\nContent-Type: multipart/form-data; boundary=XyZ\r\nConnection: close\r\nContent-Length: "
    text = text.concat(str.from(body.byte_len())?)?.concat("\r\n\r\n")?.concat(body)?
    Ok(std.bytes.from_text(&text)?)

def exhaust(files: bool) -> Result[(), Failure]:
    scratch = std.test.scratch()?
    mut table = std.http.server.router()?
    table.add("POST", "/upload", 0)?
    config = std.http.server.config()
    mut budget = 0u32
    while budget < 10000u32:
        app = Upload(std.fs.root(scratch.path())?, std.http.body.multipart_limits(), budget, files, CancellationToken()?, CancellationToken()?, CancellationToken()?)
        before = open_files()?
        stop = CancellationToken()?
        cancel = CancellationToken()?
        report = std.http.server.serve_connection(Script(request()?, 0), &table, &app, &config, &stop, &cancel)
        std.test.equal(&open_files()?, &before)?
        std.test.check(not app.wrong.is_cancelled(), "a failure was misreported")?
        if app.parsed.is_cancelled():
            std.test.check(budget > 0u32, "no injected failure was reached")?
            return Ok(())
        std.test.check(app.refused.is_cancelled(), "the handler did not finish")?
        budget += 1u32
    std.test.fail("the upload never parsed")?
    Ok(())

def test_allocation_failures_are_reported_and_release_files() -> Result[(), Failure]:
    exhaust(False)

def test_storage_failures_are_reported_and_release_files() -> Result[(), Failure]:
    exhaust(True)
"##;

#[test]
fn multipart_failures_are_reported_and_release_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("tests")).unwrap();
    std::fs::write(
        root.join("plenty.toml"),
        "manifest-version = 1\n[package]\nname = \"uploads\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/lib.plenty"),
        "pub def unused() -> i64:\n    0\n",
    )
    .unwrap();
    std::fs::write(root.join("tests/multipart.plenty"), MULTIPART).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_plenty"))
        .arg("test")
        .current_dir(root)
        .env("TMPDIR", root)
        .env_remove("PLENTY_TEST_SEED")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("test result: ok. 2 passed; 0 failed"),
        "{stdout}"
    );
}

/// The direct-TLS example from `http-proxy.md`: static files and a signed
/// session cookie served over HTTPS on loopback, with the test certificates.
const HTTPS: &str = r##"import std.bytes
import std.fs
import std.http
import std.http.cookie
import std.http.files
import std.http.server
import std.http.tls
import std.net
import std.path
import std.time
import std.tls
from std.bytes import Bytes
from std.fs import Root
from std.http.server import Exchange
from std.http.tls import TlsTransport
from std.sync import CancellationToken
from std.tls import TlsStream

pub class Site:
    pub root: Root
    pub keys: list[Bytes]

    pub def handle(self, exchange: &mut Exchange[TlsTransport]) -> Result[(), Failure]:
        if exchange.route == 0:
            path = exchange.params[0].concat("")?
            std.http.files.serve(exchange, &self.root, &path)?
        elif exchange.route == 1:
            mut head = std.http.response(204, "No Content")?
            std.http.cookie.add(&mut head, &std.http.cookie.signed_cookie(&self.keys, "session", "user42")?)?
            exchange.respond(head, &std.bytes.empty()?)?
        else:
            found = std.http.cookie.cookies(&exchange.request.headers, 8)?
            match std.http.cookie.find_signed(&found, &self.keys, "session")?:
                case Some(user):
                    exchange.send_text(200, &user)?
                case Nothing:
                    exchange.send_text(401, "sign in first")?
        Ok(())

# One request on its own TLS connection; reads until `marker` arrives,
# since the server sends no close_notify.
def ask(port: u16, request: &str, marker: &str) -> str:
    never = CancellationToken().unwrap()
    deadline = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(5u32)).unwrap()
    roots = std.tls.roots_from_pem(&read_text("FIXTURES/ca.pem").unwrap()).unwrap()
    config = std.tls.client_config(&roots).unwrap()
    tcp = std.net.connect(&std.net.loopback_v4(port), &deadline, &never).unwrap()
    mut stream = std.tls.connect(tcp, &config, "localhost", &deadline, &never).unwrap()
    data = std.bytes.from_text(request).unwrap()
    stream.write_all_before(&data, 0, data.len(), &deadline, &never).unwrap()
    mut out = std.bytes.empty().unwrap()
    while not out.to_text().unwrap().endswith(marker):
        at = out.len()
        out.resize(at + 64, 0u8).unwrap()
        count = stream.read_before(&mut out, at, at + 64, &deadline, &never).unwrap()
        out.truncate(at + count).unwrap()
    out.to_text().unwrap().replace("\r\n", "|").unwrap()

def client(port: u16, cookie: str, stop: CancellationToken) -> ():
    print(ask(port, "GET /s/index.html HTTP/1.1\r\nHost: localhost\r\n\r\n", "</h1>\n")).unwrap()
    print(ask(port, "POST /login HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n", "\r\n\r\n")).unwrap()
    request = "GET /me HTTP/1.1\r\nHost: localhost\r\nCookie: session=".concat(&cookie).unwrap().concat("\r\n\r\n").unwrap()
    print(ask(port, &request, "user42")).unwrap()
    print(ask(port, "GET /me HTTP/1.1\r\nHost: localhost\r\nCookie: session=user42\r\n\r\n", "first")).unwrap()
    stop.cancel()

def main() -> Result[(), Failure]:
    mut table = std.http.server.router()?
    table.add("GET", "/s/{path...}", 0)?
    table.add("POST", "/login", 1)?
    table.add("GET", "/me", 2)?
    keys = [std.bytes.from_text(&read_text("SESSION_KEY")?)?]?
    site = Site(std.fs.root(&std.path.from_text("SITE")?)?, keys)
    tls = std.tls.server_config(&read_text("FIXTURES/leaf.pem")?, &read_text("FIXTURES/leaf.key")?)?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    cookie = std.http.cookie.sign(&site.keys, "session", "user42")?
    config = std.http.server.config()
    with spawn(client, port, cookie, stop.share())? as task:
        report = std.http.tls.serve(listener, tls, &table, &site, &config, &stop)?
        print(report.requests)?
    Ok(())
"##;

#[test]
fn https_serves_files_and_signed_sessions_directly() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(dir.path());
    let key = dir.path().join("session.key");
    std::fs::write(&key, "k".repeat(32)).unwrap();
    let source = HTTPS
        .replace(
            "FIXTURES",
            &format!("{}/tests/fixtures/tls", env!("CARGO_MANIFEST_DIR")),
        )
        .replace("SESSION_KEY", key.to_str().unwrap())
        .replace("SITE", site.to_str().unwrap());
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, &source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{error}"));
    let output = Command::new(executable)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let tag = signature(&[b'k'; 32], b"session=user42");
    let expected = format!(
        "HTTP/1.1 200 OK|Content-Type: text/html; charset=utf-8|Content-Length: 12|X-Content-Type-Options: nosniff|ETag: {TAG}|Last-Modified: {DATE}||<h1>hi</h1>

HTTP/1.1 204 No Content|Set-Cookie: session=user42.{tag}; Path=/; Secure; HttpOnly; SameSite=Lax||
HTTP/1.1 200 OK|Content-Type: text/plain; charset=utf-8|Content-Length: 6||user42
HTTP/1.1 401 Unauthorized|Content-Type: text/plain; charset=utf-8|Content-Length: 13||sign in first
4
"
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}
