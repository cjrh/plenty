//! `std.http.websocket` and `std.http.wss`: the RFC 6455 opening handshake
//! and framing, checked byte for byte against raw sockets on each side and
//! end to end between a Plenty client and server, over TCP and TLS. Every
//! socket is on 127.0.0.1.
mod support;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use support::tls_fixtures as fixtures;

fn compile(source: &str, dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("main.plenty");
    let executable = dir.join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    executable
}

fn output(source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(compile(source, dir.path()))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{source}\nstatus {:?}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_owned()
}

/// A running Plenty program whose first output line is its port.
struct Running {
    child: Child,
    lines: BufReader<std::process::ChildStdout>,
    port: u16,
    _dir: tempfile::TempDir,
}

impl Running {
    fn start(source: &str) -> Running {
        let dir = tempfile::tempdir().unwrap();
        let mut child = Command::new(compile(source, dir.path()))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap());
        let mut first = String::new();
        lines.read_line(&mut first).unwrap();
        let port = first
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("port line {first:?}"));
        Running {
            child,
            lines,
            port,
            _dir: dir,
        }
    }

    fn line(&mut self) -> String {
        let mut line = String::new();
        self.lines.read_line(&mut line).unwrap();
        line.trim_end().to_owned()
    }

    fn finish(mut self) -> String {
        let mut rest = String::new();
        self.lines.read_to_string(&mut rest).unwrap();
        let status = self.child.wait().unwrap();
        assert!(status.success(), "status {status:?}\n{rest}");
        rest.trim_end().to_owned()
    }
}

fn connect(port: u16) -> TcpStream {
    let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream
}

/// Reads a message head, a byte at a time, with CRLFs shown as `|`.
fn head(stream: &mut TcpStream) -> String {
    let mut out = Vec::new();
    let mut one = [0u8];
    while !out.ends_with(b"\r\n\r\n") {
        if stream.read(&mut one).unwrap() == 0 {
            break;
        }
        out.push(one[0]);
    }
    String::from_utf8(out).unwrap().replace("\r\n", "|")
}

fn upgrade(path: &str, key: &str, extra: &str) -> String {
    format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nUpgrade: websocket\r\nConnection: keep-alive, Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n{extra}\r\n"
    )
}

const KEY: &str = "dGhlIHNhbXBsZSBub25jZQ==";

/// Opens a WebSocket on `path` and returns the stream after the 101 head.
fn opened(port: u16, path: &str) -> TcpStream {
    let mut stream = connect(port);
    stream.write_all(upgrade(path, KEY, "").as_bytes()).unwrap();
    let answer = head(&mut stream);
    assert!(answer.starts_with("HTTP/1.1 101 "), "{answer}");
    stream
}

fn frame(opcode: u8, payload: &[u8], fin: bool, mask: Option<[u8; 4]>) -> Vec<u8> {
    let mut out = vec![opcode | if fin { 0x80 } else { 0 }];
    let flag = if mask.is_some() { 0x80 } else { 0 };
    if payload.len() < 126 {
        out.push(flag | payload.len() as u8);
    } else if payload.len() < 65536 {
        out.push(flag | 126);
        out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        out.push(flag | 127);
        out.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    }
    match mask {
        Some(key) => {
            out.extend_from_slice(&key);
            out.extend(payload.iter().enumerate().map(|(i, b)| b ^ key[i % 4]));
        }
        None => out.extend_from_slice(payload),
    }
    out
}

fn masked(opcode: u8, payload: &[u8]) -> Vec<u8> {
    frame(opcode, payload, true, Some([0x37, 0xfa, 0x21, 0x3d]))
}

/// Reads one frame: (fin, opcode, masked, unmasked payload).
fn read_frame(stream: &mut TcpStream) -> (bool, u8, bool, Vec<u8>) {
    let mut two = [0u8; 2];
    stream.read_exact(&mut two).unwrap();
    let mut length = (two[1] & 0x7f) as u64;
    if length == 126 {
        let mut extended = [0u8; 2];
        stream.read_exact(&mut extended).unwrap();
        length = u16::from_be_bytes(extended) as u64;
    } else if length == 127 {
        let mut extended = [0u8; 8];
        stream.read_exact(&mut extended).unwrap();
        length = u64::from_be_bytes(extended);
    }
    let masked = two[1] & 0x80 != 0;
    let mut key = [0u8; 4];
    if masked {
        stream.read_exact(&mut key).unwrap();
    }
    let mut payload = vec![0u8; length as usize];
    stream.read_exact(&mut payload).unwrap();
    if masked {
        for (i, byte) in payload.iter_mut().enumerate() {
            *byte ^= key[i % 4];
        }
    }
    (two[0] & 0x80 != 0, two[0] & 0x0f, masked, payload)
}

#[test]
fn reference_accept_value() {
    assert_eq!(accept_value(KEY), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
}

fn close_code(payload: &[u8]) -> u16 {
    u16::from_be_bytes([payload[0], payload[1]])
}

fn at_end(stream: &mut TcpStream) -> bool {
    let mut rest = Vec::new();
    stream.read_to_end(&mut rest).unwrap();
    rest.is_empty()
}

const SERVER: &str = r##"import std.bytes
import std.http
import std.http.server
import std.http.websocket as ws
import std.net
import std.time
from std.bytes import Bytes
from std.http.server import Exchange
from std.http.websocket import Kind, Message, WebSocketError
from std.net import TcpStream
from std.sync import CancellationToken
from std.time import Deadline

pub enum Route:
    Echo
    Wait
    Stop

pub class App:
    pub stop: CancellationToken

    pub def handle(self, route: &Route, exchange: &mut Exchange[TcpStream]) -> Result[(), WebSocketError | std.bytes.BytesError | std.http.HttpError | RangeError | AllocError | IoError]:
        mut config = ws.config()
        config.max_frame = 1024
        config.max_message = 4096
        config.subprotocol = Some("chat")
        match route:
            case Route.Echo:
                match ws.accept(exchange, &config):
                    case Ok(socket):
                        mut owned = socket
                        print(echo(&mut owned)?)?
                    case Err(error):
                        print(error)?
            case Route.Wait:
                mut socket = ws.accept(exchange, &config)?
                mut buffer = std.bytes.empty()?
                match socket.receive(&mut buffer, &later()?):
                    case Err(WebSocketError.Stopping):
                        print("stopping")?
                        socket.close(1001, "going away", &later()?)?
                    case _ as other:
                        print(str.repr(&other)?)?
            case Route.Stop:
                exchange.send_text(200, "stopping")?
                self.stop.cancel()
        Ok(())

def later() -> Result[Deadline, RangeError]:
    Ok(std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(10u32))?)

# Echoes messages until the peer closes; returns how the connection ended.
def echo(socket: &mut ws.WebSocket[TcpStream]) -> Result[str, WebSocketError | std.bytes.BytesError | RangeError | AllocError]:
    mut buffer = std.bytes.empty()?
    while True:
        buffer.clear()
        match socket.receive(&mut buffer, &later()?):
            case Ok(Message.Text):
                socket.send(Kind.Text, &buffer, 0, buffer.len(), &later()?)?
            case Ok(Message.Binary):
                socket.send(Kind.Binary, &buffer, 0, buffer.len(), &later()?)?
            case Ok(Message.Close(code)):
                return Ok("closed ".concat(&str.from(code)?)?.concat(" ")?.concat(&buffer.to_text()?)?)
            case Err(error):
                return Ok(str.repr(&error)?)
    Ok("")

def main() -> Result[(), std.net.NetError | std.http.server.ServerError | AllocError | IoError]:
    mut table = std.http.server.router[Route]()?
    table.add("GET", "/echo", Route.Echo)?
    table.add("GET", "/wait", Route.Wait)?
    table.add("GET", "/stop", Route.Stop)?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    print(listener.local_address()?.port)?
    stop = CancellationToken()?
    report = std.http.server.serve(listener, &table, &App(stop.share()), &std.http.server.config(), &stop)?
    print("handler_errors=".concat(&str.from(report.handler_errors)?)?)?
    Ok(())
"##;

/// The server answers the RFC 6455 sample key, picks the subprotocol it was
/// configured with, unmasks fragmented and pipelined frames, answers pings
/// between fragments, echoes the closing handshake, and closes with 1002,
/// 1009 and 1007 for unmasked frames, oversized frames and invalid text.
/// Requests that are not handshakes are refused with 426 or 400. On server
/// shutdown, a handler waiting for a message sees Stopping.
#[test]
fn server_handshake_framing_and_close_codes() {
    let mut server = Running::start(SERVER);
    let port = server.port;

    // Handshake, with a text frame sent in the same write as the request.
    let mut stream = connect(port);
    let mut request =
        upgrade("/echo", KEY, "Sec-WebSocket-Protocol: superchat, chat\r\n").into_bytes();
    request.extend(masked(1, b"early"));
    stream.write_all(&request).unwrap();
    assert_eq!(
        head(&mut stream),
        "HTTP/1.1 101 Switching Protocols|Upgrade: websocket|Connection: Upgrade|Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=|Sec-WebSocket-Protocol: chat||"
    );
    assert_eq!(read_frame(&mut stream), (true, 1, false, b"early".to_vec()));

    // A fragmented message with a ping between its fragments.
    let mut burst = frame(1, b"Hel", false, Some([1, 2, 3, 4]));
    burst.extend(masked(9, b"are you there"));
    burst.extend(frame(0, b"lo", true, Some([5, 6, 7, 8])));
    stream.write_all(&burst).unwrap();
    assert_eq!(
        read_frame(&mut stream),
        (true, 10, false, b"are you there".to_vec())
    );
    assert_eq!(read_frame(&mut stream), (true, 1, false, b"Hello".to_vec()));

    // A binary message of 300 bytes uses the 16-bit length.
    let data: Vec<u8> = (0..300).map(|i| i as u8).collect();
    stream.write_all(&masked(2, &data)).unwrap();
    assert_eq!(read_frame(&mut stream), (true, 2, false, data));

    // The closing handshake.
    let mut close = vec![0x03, 0xe8];
    close.extend_from_slice(b"done");
    stream.write_all(&masked(8, &close)).unwrap();
    let (_, opcode, _, payload) = read_frame(&mut stream);
    assert_eq!((opcode, close_code(&payload)), (8, 1000));
    assert!(at_end(&mut stream));
    assert_eq!(server.line(), "closed 1000 done");

    // Protocol faults close with their codes.
    for (bytes, code, error) in [
        (frame(1, b"bare", true, None), 1002, "Protocol"),
        (masked(2, &[0u8; 2000]), 1009, "TooLarge"),
        (masked(1, &[0xff, 0xfe]), 1007, "InvalidText"),
        (masked(8, &[0x03]), 1002, "Protocol"),
        (masked(0, b"orphan"), 1002, "Protocol"),
        (masked(0x41, b"rsv"), 1002, "Protocol"),
    ] {
        let mut stream = opened(port, "/echo");
        stream.write_all(&bytes).unwrap();
        let (_, opcode, _, payload) = read_frame(&mut stream);
        assert_eq!((opcode, close_code(&payload)), (8, code), "{error}");
        assert!(at_end(&mut stream));
        assert_eq!(
            server.line(),
            format!("std.http.websocket.WebSocketError.{error}")
        );
    }

    // Refusals.
    for (request, status, line) in [
        (
            "GET /echo HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n".to_owned(),
            "HTTP/1.1 426 Upgrade Required|",
            "Refused(426)",
        ),
        (
            upgrade("/echo", "c2hvcnQ=", ""),
            "HTTP/1.1 400 Bad Request|",
            "Refused(400)",
        ),
        (
            upgrade("/echo", KEY, "").replace("Version: 13", "Version: 8"),
            "HTTP/1.1 426 Upgrade Required|",
            "Refused(426)",
        ),
    ] {
        let mut stream = connect(port);
        stream.write_all(request.as_bytes()).unwrap();
        let answer = head(&mut stream);
        assert!(answer.starts_with(status), "{answer}");
        if status.contains("426") {
            assert!(
                answer.contains("|Upgrade: websocket|Sec-WebSocket-Version: 13|"),
                "{answer}"
            );
        }
        assert_eq!(
            server.line(),
            format!("std.http.websocket.WebSocketError.{line}")
        );
    }

    // Shutdown ends a wait for the next message.
    let mut waiting = opened(port, "/wait");
    let mut stop = connect(port);
    stop.write_all(b"GET /stop HTTP/1.1\r\nHost: x\r\n\r\n")
        .unwrap();
    let (_, opcode, _, payload) = read_frame(&mut waiting);
    assert_eq!((opcode, close_code(&payload)), (8, 1001));
    assert_eq!(&payload[2..], b"going away");
    waiting.write_all(&masked(8, &[0x03, 0xe9])).unwrap();
    assert!(at_end(&mut waiting));
    assert_eq!(server.line(), "stopping");
    assert_eq!(server.finish(), "handler_errors=0");
}

const CLIENT: &str = r##"import std.bytes
import std.http.websocket as ws
import std.time
from std.bytes import Bytes
from std.http.websocket import Kind, Message, WebSocketError
from std.sync import CancellationToken
from std.time import Deadline

def later() -> Result[Deadline, RangeError]:
    Ok(std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(10u32))?)

def url(path: &str) -> Result[str, AllocError]:
    Ok("ws://localhost:PORT".concat(path)?)

def session() -> Result[(), WebSocketError | std.bytes.BytesError | RangeError | AllocError | IoError]:
    never = CancellationToken()?
    mut config = ws.config()
    config.max_frame = 100000
    config.subprotocol = Some("chat")
    mut socket = ws.connect(&url("/chat?room=1")?, &config, &later()?, &never)?
    print(socket.subprotocol)?
    mut buffer = std.bytes.empty()?
    print(socket.receive(&mut buffer, &later()?)?)?
    print(buffer.to_text()?)?
    mut data = std.bytes.with_capacity(70000)?
    for index in range(70000):
        data.push(u8(index % 251))?
    socket.send(Kind.Binary, &data, 0, data.len(), &later()?)?
    buffer.clear()
    print(socket.receive(&mut buffer, &later()?)?)?
    print(buffer.to_text()?)?
    print(socket.is_open())?
    print(str.repr(&socket.send_text("late", &later()?))?)?
    Ok(())

def attempt(path: &str) -> Result[(), std.bytes.BytesError | RangeError | AllocError | IoError]:
    never = CancellationToken()?
    match ws.connect(&url(path)?, &ws.config(), &later()?, &never):
        case Ok(socket):
            mut owned = socket
            mut buffer = std.bytes.empty()?
            print(str.repr(&owned.receive(&mut buffer, &later()?))?)?
        case Err(error):
            print(error)?
    Ok(())

def main() -> Result[(), WebSocketError | std.bytes.BytesError | RangeError | AllocError | IoError]:
    session()?
    attempt("/forbidden")?
    attempt("/wrong-accept")?
    attempt("/masked")?
    Ok(())
"##;

/// SHA-1 (FIPS 180-4), for computing accept values independently of the
/// runtime's implementation.
fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in message.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(block[i * 4..i * 4 + 4].try_into().unwrap());
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, word) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let next = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = next;
        }
        for (slot, value) in h.iter_mut().zip([a, b, c, d, e]) {
            *slot = slot.wrapping_add(value);
        }
    }
    let mut out = [0u8; 20];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(data: &[u8]) -> String {
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let bits = (chunk[0] as u32) << 16
            | (*chunk.get(1).unwrap_or(&0) as u32) << 8
            | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(BASE64[(bits >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn decoded_len(text: &str) -> usize {
    text.len() / 4 * 3 - text.bytes().rev().take_while(|b| *b == b'=').count()
}

fn accept_value(key: &str) -> String {
    base64(&sha1(
        format!("{key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11").as_bytes(),
    ))
}

fn field<'a>(head: &'a str, name: &str) -> &'a str {
    head.split('|')
        .find_map(|line| {
            let (key, value) = line.split_once(": ")?;
            key.eq_ignore_ascii_case(name).then_some(value)
        })
        .unwrap_or_else(|| panic!("{name} in {head}"))
}

/// The client sends a masked handshake with a fresh 16-byte key and the
/// URL's resource, verifies the server's accept value, reassembles a
/// fragmented message while answering a ping, masks a 70000-byte message
/// sent with the 64-bit length, and completes a server-initiated close.
/// A non-101 status, a wrong accept value and a masked server frame fail.
#[test]
fn client_handshake_masking_and_failures() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let source = CLIENT.replace("PORT", &port.to_string());
    let server = std::thread::spawn(move || {
        let mut heads = Vec::new();
        // The session.
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let request = head(&mut stream);
        let key = field(&request, "Sec-WebSocket-Key").to_owned();
        heads.push(request.replace(&key, "KEY"));
        assert_eq!(decoded_len(&key), 16, "{key}");
        let mut answer = format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: WebSocket\r\nConnection: upgrade\r\nSec-WebSocket-Accept: {}\r\nSec-WebSocket-Protocol: chat\r\n\r\n",
            accept_value(&key)
        )
        .into_bytes();
        answer.extend(frame(1, b"hel", false, None));
        answer.extend(frame(9, b"ping!", true, None));
        answer.extend(frame(0, b"lo", true, None));
        stream.write_all(&answer).unwrap();
        let pong = read_frame(&mut stream);
        assert_eq!(pong, (true, 10, true, b"ping!".to_vec()));
        let (fin, opcode, was_masked, payload) = read_frame(&mut stream);
        assert!(fin && was_masked);
        assert_eq!(opcode, 2);
        assert_eq!(payload.len(), 70000);
        assert!(
            payload
                .iter()
                .enumerate()
                .all(|(i, b)| *b == (i % 251) as u8)
        );
        let mut close = vec![0x0f, 0xa0];
        close.extend_from_slice(b"bye");
        stream.write_all(&frame(8, &close, true, None)).unwrap();
        let (_, opcode, was_masked, payload) = read_frame(&mut stream);
        // The answer echoes the code, not the reason.
        assert_eq!(
            (opcode, was_masked, payload),
            (8, true, close[..2].to_vec())
        );
        assert!(at_end(&mut stream));

        // The failures.
        for _ in 0..3 {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let request = head(&mut stream);
            let key = field(&request, "Sec-WebSocket-Key").to_owned();
            let path = request.split(' ').nth(1).unwrap().to_owned();
            let reply = match path.as_str() {
                "/forbidden" => "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n".to_owned().into_bytes(),
                "/wrong-accept" => b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\r\n".to_vec(),
                _ => {
                    let mut reply = format!(
                        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
                        accept_value(&key)
                    )
                    .into_bytes();
                    reply.extend(masked(1, b"masked"));
                    reply
                }
            };
            stream.write_all(&reply).unwrap();
            if path == "/masked" {
                let (_, opcode, _, payload) = read_frame(&mut stream);
                assert_eq!((opcode, close_code(&payload)), (8, 1002));
            }
        }
        heads
    });
    let printed = output(&source);
    let heads = server.join().unwrap();
    assert_eq!(
        heads[0],
        format!(
            "GET /chat?room=1 HTTP/1.1|Host: localhost:{port}|Upgrade: websocket|Connection: Upgrade|Sec-WebSocket-Key: KEY|Sec-WebSocket-Version: 13|Sec-WebSocket-Protocol: chat||"
        )
    );
    assert_eq!(
        printed,
        "Option[str].Some(\"chat\")
std.http.websocket.Message.Text
hello
std.http.websocket.Message.Close(4000)
bye
False
Result[(), std.http.websocket.WebSocketError].Err(std.http.websocket.WebSocketError.Closed)
std.http.websocket.WebSocketError.Rejected(403)
std.http.websocket.WebSocketError.InvalidHandshake
Result[std.http.websocket.Message, std.http.websocket.WebSocketError].Err(std.http.websocket.WebSocketError.Protocol)"
    );
}

/// `target` accepts `ws` and `wss` URLs with their default ports, bracketed
/// IPv6 hosts and queries, and rejects other schemes, credentials,
/// fragments and port 0; `connect` leaves `wss` to std.http.wss.
#[test]
fn targets_parse_ws_and_wss_urls() {
    let printed = output(
        r##"import std.http.websocket as ws
import std.time
from std.sync import CancellationToken

def show(url: &str) -> Result[(), AllocError | IoError]:
    match ws.target(url):
        case Ok(found):
            print(found.secure, found.host, found.port)?
        case Err(error):
            print(error)?
    Ok(())

def main() -> Result[(), RangeError | AllocError | IoError]:
    show("ws://Example.COM/chat")?
    show("wss://example.com:8443/a?b=c")?
    show("ws://[::1]:9000/")?
    show("http://example.com/")?
    show("ws://user@example.com/")?
    show("ws://example.com/#top")?
    show("ws://example.com:0/")?
    never = CancellationToken()?
    later = std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(1u32))?
    match ws.connect("wss://example.com/", &ws.config(), &later, &never):
        case Ok(_):
            print("connected")?
        case Err(error):
            print(error)?
    Ok(())
"##,
    );
    assert_eq!(
        printed,
        "False example.com 80
True example.com 8443
False ::1 9000
std.http.websocket.WebSocketError.UnsupportedScheme
std.http.websocket.WebSocketError.InvalidUrl
std.http.websocket.WebSocketError.InvalidUrl
std.http.websocket.WebSocketError.InvalidUrl
std.http.websocket.WebSocketError.UnsupportedScheme"
    );
}

/// A Plenty client and server talk over TLS: std.http.tls serves the
/// exchange that std.http.websocket upgrades, and std.http.wss verifies the
/// server against the fixture CA. Small frames split each message, and the
/// client closes. A client trusting another CA fails the TLS handshake.
#[test]
fn wss_connects_plenty_client_and_server() {
    let source = r##"import std.bytes
import std.http
import std.http.server
import std.http.tls
import std.http.websocket as ws
import std.http.wss
import std.net
import std.time
import std.tls
from std.bytes import Bytes
from std.http.server import Exchange
from std.http.tls import TlsTransport
from std.http.websocket import Kind, Message, WebSocketError
from std.sync import CancellationToken
from std.time import Deadline
from std.tls import ClientConfig

pub enum Route:
    Echo

pub class App:
    pub def handle(self, route: &Route, exchange: &mut Exchange[TlsTransport]) -> Result[(), WebSocketError | std.bytes.BytesError | RangeError | AllocError | IoError]:
        mut config = ws.config()
        config.max_frame = 4
        mut socket = ws.accept(exchange, &config)?
        mut buffer = std.bytes.empty()?
        while True:
            buffer.clear()
            match socket.receive(&mut buffer, &later()?)?:
                case Message.Text:
                    reply = "echo: ".concat(&buffer.to_text()?)?
                    socket.send_text(&reply, &later()?)?
                case Message.Binary:
                    pass
                case Message.Close(code):
                    print("server saw", code, buffer.to_text()?)?
                    return Ok(())
        Ok(())

def later() -> Result[Deadline, RangeError]:
    Ok(std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(10u32))?)

def trusting(name: &str) -> Result[ClientConfig, AllocError | IoError]:
    Ok(std.tls.client_config(&std.tls.roots_from_pem(&read_text(&("FIXTURES/" + name)?)?).unwrap()).unwrap())

def talk(port: u16) -> Result[(), WebSocketError | std.bytes.BytesError | RangeError | AllocError | IoError]:
    never = CancellationToken()?
    url = "wss://localhost:".concat(&str.from(i64(port))?)?.concat("/echo")?
    mut config = ws.config()
    config.max_frame = 4
    mut socket = std.http.wss.connect(&url, &trusting("ca.pem")?, &config, &later()?, &never)?
    socket.send_text("over tls", &later()?)?
    mut buffer = std.bytes.empty()?
    print(socket.receive(&mut buffer, &later()?)?, buffer.to_text()?)?
    socket.close(1000, "done", &later()?)?
    print(socket.is_open())?
    print(std.http.wss.connect(&url, &trusting("other-ca.pem")?, &config, &later()?, &never).is_err())?
    Ok(())

def client(port: u16, stop: CancellationToken) -> ():
    match talk(port):
        case Ok(_):
            pass
        case Err(error):
            print(error).unwrap()
    stop.cancel()

def main() -> Result[(), std.net.NetError | std.http.server.ServerError | ThreadError | AllocError | IoError]:
    mut table = std.http.server.router[Route]()?
    table.add("GET", "/echo", Route.Echo)?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    tls = std.tls.server_config(&read_text("FIXTURES/leaf.pem")?, &read_text("FIXTURES/leaf.key")?).unwrap()
    stop = CancellationToken()?
    with spawn(client, port, stop.share())? as task:
        report = std.http.tls.serve(listener, tls, &table, &App(), &std.http.server.config(), &stop)?
        print("handler_errors", report.handler_errors)?
    Ok(())
"##
    .replace("FIXTURES", &fixtures());
    // The server thread's line races the client's.
    let printed = output(&source);
    let mut lines: Vec<&str> = printed.lines().collect();
    lines.sort();
    assert_eq!(
        lines,
        [
            "False",
            "True",
            "handler_errors 0",
            "server saw 1000 done",
            "std.http.websocket.Message.Text echo: over tls",
        ],
        "{printed}"
    );
}
