//! `std.http.client`: pooling, replay, redirects, proxies and deadlines.
//! Scripted connections run in memory through a test `Connector`; the rest
//! use loopback sockets on ephemeral ports. No test needs a network.

/// An in-memory `Transport` and `Connector` that echo what the client sends.
const SCRIPTED: &str = r##"import std.http.client as hc
import std.bytes
import std.http
import std.net
import std.time
from std.bytes import ByteSource, Bytes
from std.http.client import ClientError, Endpoint, Request
from std.http import HttpError
from std.net import NetError
from std.sync import CancellationToken
from std.time import Deadline

# One scripted connection. `input` is cut at `ends` into replies; a write
# releases the next reply once the previous one has been read. With every
# reply released and read, a read reports end of input, or a timeout when
# `hang` is set; before then it reports a timeout, as an idle live peer
# does. Written bytes are copied to standard output when `echo` is set. A write fails with
# `Reset` once `fail_after` bytes have gone, when it is not negative.
pub class Script:
    pub input: Bytes
    pub ends: list[i64]
    pub released: i64
    pub at: i64
    pub hang: bool
    pub fail_after: i64
    pub sent: i64
    pub echo: bool

    pub def read_before(self: &mut Script, buffer: &mut Bytes, start: i64, stop: i64, deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, ClientError]:
        if cancel.is_cancelled():
            return Err(ClientError.Net(NetError.Cancelled))
        mut end = 0
        if self.released > 0:
            end = self.ends[self.released - 1]
        left = end - self.at
        if left == 0:
            if self.released == len(self.ends) and not self.hang:
                return Ok(0)
            return Err(ClientError.Net(NetError.TimedOut))
        mut count = stop - start
        if count > left:
            count = left
        buffer.overwrite(start, &self.input, self.at, self.at + count).unwrap()
        self.at += count
        Ok(count)

    pub def write_before[S: ByteSource](self: &mut Script, source: &S, start: i64, stop: i64, deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, ClientError]:
        if cancel.is_cancelled():
            return Err(ClientError.Net(NetError.Cancelled))
        mut count = stop - start
        if self.fail_after >= 0:
            if self.sent >= self.fail_after:
                return Err(ClientError.Net(NetError.Reset))
            if count > self.fail_after - self.sent:
                count = self.fail_after - self.sent
        if self.echo:
            source.write_stdout(start, start + count).unwrap()
        self.sent += count
        mut end = 0
        if self.released > 0:
            end = self.ends[self.released - 1]
        if self.at == end and self.released < len(self.ends):
            self.released += 1
        Ok(count)

# Hands out scripted connections in order, printing each endpoint when
# `echo` is set.
pub class Scripts:
    pub queue: list[Script]
    pub echo: bool

    pub def open(self: &mut Scripts, endpoint: &Endpoint, deadline: &Deadline, cancel: &CancellationToken) -> Result[Script, ClientError]:
        if self.echo:
            print(described(endpoint).unwrap()).unwrap()
        if len(self.queue) == 0:
            return Err(ClientError.Net(NetError.Refused))
        match self.queue.pop(0):
            case Some(script):
                Ok(script)
            case Nothing:
                Err(ClientError.Net(NetError.Refused))

def described(endpoint: &Endpoint) -> Result[str, Failure]:
    Ok("[connect ".concat(&endpoint.scheme)?.concat("://")?.concat(&endpoint.host)?.concat(":")?.concat(&str.from(i64(endpoint.port))?)?.concat("]")?)

# A connection that answers with each of `replies` in turn, then closes.
pub def script(replies: list[str], hang: bool) -> Result[Script, Failure]:
    mut input = std.bytes.empty()?
    mut ends = list[i64].new()?
    for index in range(len(replies)):
        input.extend(&std.bytes.from_text(&replies[index])?)?
        ends.append(input.len())?
    Ok(Script(input, ends, 0, 0, hang, -1, 0, True))

pub def scripted(connections: list[Script], config: hc.Config) -> Result[hc.Client[Script, Scripts], Failure]:
    Ok(hc.connector[Script, Scripts](Scripts(connections, True), config)?)

pub def later() -> Result[Deadline, Failure]:
    Ok(std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(5u32))?)

pub def show(result: Result[hc.Fetched, ClientError]) -> Result[(), Failure]:
    print("")?
    match result:
        case Ok(fetched):
            line = "=> ".concat(&str.from(fetched.status)?)?.concat(" ")?.concat(&fetched.url)?.concat(" redirects=")?.concat(&str.from(fetched.redirects)?)?.concat(" body=")?.concat(&fetched.body.to_text()?)?
            print(line)?
        case Err(error):
            print("=> error")?
            print(error)?
    Ok(())

pub def stats[T, C](client: &hc.Client[T, C]) -> Result[(), Failure]:
    s = client.stats()
    line = "opened=".concat(&str.from(s.opened)?)?.concat(" reused=")?.concat(&str.from(s.reused)?)?.concat(" retried=")?.concat(&str.from(s.retried)?)?.concat(" redirected=")?.concat(&str.from(s.redirected)?)?.concat(" discarded=")?.concat(&str.from(s.discarded)?)?.concat(" idle=")?.concat(&str.from(client.idle())?)?
    print(line)?
    Ok(())

def ok(body: &str) -> Result[str, Failure]:
    Ok("HTTP/1.1 200 OK\r\nContent-Length: ".concat(&str.from(body.byte_len())?)?.concat("\r\n\r\n")?.concat(body)?)

"##;

fn output(source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let output = std::process::Command::new(executable)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{source}\nstatus {:?}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .replace("\r\n", "|")
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_owned()
}

fn runs(prelude: &str, main: &str, expected: &str) {
    assert_eq!(output(&format!("{prelude}{main}")), expected.trim_end());
}

/// One connection carries two requests; a connection the server closed while
/// idle, an unfinished response, `max_idle = 0` and a closing response each
/// lead to a new connection; the pool holds `max_idle_per_origin` per origin.
#[test]
fn pooled_connections_are_reused_and_closed_ones_replaced() {
    runs(
        SCRIPTED,
        r##"
def main() -> Result[(), Failure]:
    never = CancellationToken()?
    first = script([ok("one")?, ok("two")?]?, True)?
    mut client = scripted([first]?, hc.config()?)?
    show(client.get("http://Example.com/a?x=1", &later()?, &never))?
    mut put = hc.request("PUT", "http://example.com:80/b")?
    put.add("X-Trace", "7")?
    put.body = std.bytes.from_text("data")?
    show(client.fetch(put, &later()?, &never))?
    stats(&client)?
    client.close_idle()
    stats(&client)?
    print("--- a closed idle connection is replaced")?
    a = script([ok("a")?]?, False)?
    b = script([ok("b")?]?, True)?
    mut second = scripted([a, b]?, hc.config()?)?
    show(second.get("http://h/1", &later()?, &never))?
    show(second.get("http://h/2", &later()?, &never))?
    stats(&second)?
    print("--- an unfinished response closes its connection")?
    c = script([ok("long body")?]?, True)?
    d = script([ok("d")?]?, True)?
    mut third = scripted([c, d]?, hc.config()?)?
    response = third.send(hc.request("GET", "http://h/1")?, &later()?, &never)?
    print(response.status)?
    drop(response)
    show(third.get("http://h/2", &later()?, &never))?
    stats(&third)?
    print("--- max_idle 0 asks the server to close")?
    e = script([ok("e")?]?, True)?
    f = script([ok("f")?]?, True)?
    mut config = hc.config()?
    config.max_idle = 0
    mut fourth = scripted([e, f]?, config)?
    show(fourth.get("http://h/1", &later()?, &never))?
    show(fourth.get("http://h/2", &later()?, &never))?
    stats(&fourth)?
    print("--- a response that closes is not pooled")?
    g = script(["HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 1\r\n\r\ng"]?, True)?
    h = script(["HTTP/1.0 200 OK\r\n\r\nuntil close"]?, False)?
    mut fifth = scripted([g, h]?, hc.config()?)?
    show(fifth.get("http://h/1", &later()?, &never))?
    show(fifth.get("http://h/2", &later()?, &never))?
    stats(&fifth)?
    print("--- the pool keeps max_idle_per_origin connections")?
    i = script([ok("i")?]?, True)?
    j = script([ok("j")?]?, True)?
    mut config6 = hc.config()?
    config6.max_idle_per_origin = 1
    mut sixth = scripted([i, j]?, config6)?
    mut one = sixth.send(hc.request("GET", "http://h/1")?, &later()?, &never)?
    mut two = sixth.send(hc.request("GET", "http://h/2")?, &later()?, &never)?
    mut both = std.bytes.empty()?
    one.read_all(&mut both)?
    two.read_all(&mut both)?
    print(both.to_text()?)?
    sixth.finish(one)
    sixth.finish(two)
    stats(&sixth)?
    Ok(())
"##,
        r##"[connect http://example.com:80]
GET /a?x=1 HTTP/1.1|Host: Example.com||
=> 200 http://Example.com/a?x=1 redirects=0 body=one
PUT /b HTTP/1.1|Host: example.com:80|X-Trace: 7|Content-Length: 4||data
=> 200 http://example.com:80/b redirects=0 body=two
opened=1 reused=1 retried=0 redirected=0 discarded=0 idle=1
opened=1 reused=1 retried=0 redirected=0 discarded=0 idle=0
--- a closed idle connection is replaced
[connect http://h:80]
GET /1 HTTP/1.1|Host: h||
=> 200 http://h/1 redirects=0 body=a
[connect http://h:80]
GET /2 HTTP/1.1|Host: h||
=> 200 http://h/2 redirects=0 body=b
opened=2 reused=0 retried=0 redirected=0 discarded=1 idle=1
--- an unfinished response closes its connection
[connect http://h:80]
GET /1 HTTP/1.1|Host: h||200
[connect http://h:80]
GET /2 HTTP/1.1|Host: h||
=> 200 http://h/2 redirects=0 body=d
opened=2 reused=0 retried=0 redirected=0 discarded=0 idle=1
--- max_idle 0 asks the server to close
[connect http://h:80]
GET /1 HTTP/1.1|Host: h|Connection: close||
=> 200 http://h/1 redirects=0 body=e
[connect http://h:80]
GET /2 HTTP/1.1|Host: h|Connection: close||
=> 200 http://h/2 redirects=0 body=f
opened=2 reused=0 retried=0 redirected=0 discarded=0 idle=0
--- a response that closes is not pooled
[connect http://h:80]
GET /1 HTTP/1.1|Host: h||
=> 200 http://h/1 redirects=0 body=g
[connect http://h:80]
GET /2 HTTP/1.1|Host: h||
=> 200 http://h/2 redirects=0 body=until close
opened=2 reused=0 retried=0 redirected=0 discarded=0 idle=0
--- the pool keeps max_idle_per_origin connections
[connect http://h:80]
GET /1 HTTP/1.1|Host: h||[connect http://h:80]
GET /2 HTTP/1.1|Host: h||ij
opened=2 reused=0 retried=0 redirected=0 discarded=1 idle=1"##,
    );
}

/// A GET that finds its reused connection closed before any response byte is
/// replayed once on a new connection; POST, `retries = 0`, a failure on a new
/// connection and a failure after response bytes are reported instead.
#[test]
fn only_idempotent_requests_fail_over_to_a_new_connection() {
    runs(
        SCRIPTED,
        r##"
def main() -> Result[(), Failure]:
    never = CancellationToken()?
    # The server reads the second request and closes without answering.
    a = script([ok("a")?, ""]?, False)?
    b = script([ok("b")?]?, True)?
    mut client = scripted([a, b]?, hc.config()?)?
    show(client.get("http://h/1", &later()?, &never))?
    show(client.get("http://h/2", &later()?, &never))?
    stats(&client)?
    print("--- POST is never replayed")?
    c = script([ok("c")?, ""]?, False)?
    d = script([ok("d")?]?, True)?
    mut second = scripted([c, d]?, hc.config()?)?
    show(second.get("http://h/1", &later()?, &never))?
    mut post = hc.request("POST", "http://h/2")?
    post.body = std.bytes.from_text("x")?
    show(second.fetch(post, &later()?, &never))?
    stats(&second)?
    print("--- retries 0 replays nothing")?
    e = script([ok("e")?, ""]?, False)?
    mut config = hc.config()?
    config.retries = 0
    mut third = scripted([e]?, config)?
    show(third.get("http://h/1", &later()?, &never))?
    show(third.get("http://h/2", &later()?, &never))?
    stats(&third)?
    print("--- a failure on a new connection is not replayed")?
    mut f = script([ok("f")?]?, True)?
    f.fail_after = 10
    mut fourth = scripted([f]?, hc.config()?)?
    show(fourth.get("http://h/1", &later()?, &never))?
    stats(&fourth)?
    print("--- a failure after response bytes is not replayed")?
    g = script([ok("g")?, "HTTP/1.1 200 OK\r\nContent-Le"]?, False)?
    mut fifth = scripted([g]?, hc.config()?)?
    show(fifth.get("http://h/1", &later()?, &never))?
    show(fifth.get("http://h/2", &later()?, &never))?
    stats(&fifth)?
    Ok(())
"##,
        r##"[connect http://h:80]
GET /1 HTTP/1.1|Host: h||
=> 200 http://h/1 redirects=0 body=a
GET /2 HTTP/1.1|Host: h||[connect http://h:80]
GET /2 HTTP/1.1|Host: h||
=> 200 http://h/2 redirects=0 body=b
opened=2 reused=1 retried=1 redirected=0 discarded=0 idle=1
--- POST is never replayed
[connect http://h:80]
GET /1 HTTP/1.1|Host: h||
=> 200 http://h/1 redirects=0 body=c
POST /2 HTTP/1.1|Host: h|Content-Length: 1||x
=> error
std.http.client.ClientError.Http(std.http.HttpError.Truncated)
opened=1 reused=1 retried=0 redirected=0 discarded=0 idle=0
--- retries 0 replays nothing
[connect http://h:80]
GET /1 HTTP/1.1|Host: h||
=> 200 http://h/1 redirects=0 body=e
GET /2 HTTP/1.1|Host: h||
=> error
std.http.client.ClientError.Http(std.http.HttpError.Truncated)
opened=1 reused=1 retried=0 redirected=0 discarded=0 idle=0
--- a failure on a new connection is not replayed
[connect http://h:80]
GET /1 HTT
=> error
std.http.client.ClientError.Net(std.net.NetError.Reset)
opened=1 reused=0 retried=0 redirected=0 discarded=0 idle=0
--- a failure after response bytes is not replayed
[connect http://h:80]
GET /1 HTTP/1.1|Host: h||
=> 200 http://h/1 redirects=0 body=g
GET /2 HTTP/1.1|Host: h||
=> error
std.http.client.ClientError.Http(std.http.HttpError.Truncated)
opened=1 reused=1 retried=0 redirected=0 discarded=0 idle=0"##,
    );
}

/// Streamed request bodies: `Content-Length` and chunked framing, a length
/// that does not match, a failing source, `Expect: 100-continue` answered,
/// refused and unanswered, replay only before any body byte is read, and
/// redirects that would need the body again.
#[test]
fn streamed_request_bodies_and_100_continue() {
    runs(
        SCRIPTED,
        r##"
from std.io import StreamError

# Yields `data` in reads of at most `step` bytes, then ends, or fails when
# `fail` is set.
pub class Pieces:
    pub data: Bytes
    pub at: i64
    pub step: i64
    pub fail: bool

    pub def read(self: &mut Pieces, buffer: &mut Bytes, start: i64, stop: i64) -> Result[i64, StreamError]:
        left = self.data.len() - self.at
        if left == 0:
            if self.fail:
                return Err(StreamError.Io(IoError.Other))
            return Ok(0)
        mut count = stop - start
        if count > self.step:
            count = self.step
        if count > left:
            count = left
        buffer.overwrite(start, &self.data, self.at, self.at + count).unwrap()
        self.at += count
        Ok(count)

def pieces(text: &str) -> Result[Pieces, Failure]:
    Ok(Pieces(std.bytes.from_text(text)?, 0, 4, False))

def done(result: Result[hc.Response[Script], ClientError], client: &mut hc.Client[Script, Scripts]) -> Result[(), Failure]:
    print("")?
    match result:
        case Ok(response):
            mut owned = response
            mut body = std.bytes.empty()?
            owned.read_all(&mut body)?
            print("=> ".concat(&str.from(owned.status)?)?.concat(" ")?.concat(&owned.url)?.concat(" redirects=")?.concat(&str.from(owned.redirects)?)?.concat(" body=")?.concat(&body.to_text()?)?)?
            client.finish(owned)
        case Err(error):
            print("=> error")?
            print(error)?
    Ok(())

def main() -> Result[(), Failure]:
    never = CancellationToken()?
    a = script([ok("s1")?, ok("c1")?]?, True)?
    mut client = scripted([a]?, hc.config()?)?
    mut source = pieces("hello world")?
    done(client.send_from(hc.request("PUT", "http://h/sized")?, &mut source, &hc.sized(11), &later()?, &never), &mut client)?
    source = pieces("hello world")?
    done(client.send_from(hc.request("POST", "http://h/chunked")?, &mut source, &hc.chunked(), &later()?, &never), &mut client)?
    stats(&client)?
    print("--- the length must match")?
    b = script([ok("x")?]?, True)?
    c = script([ok("x")?]?, True)?
    d = script([ok("x")?]?, True)?
    mut checked = scripted([b, c, d]?, hc.config()?)?
    source = pieces("hello world")?
    done(checked.send_from(hc.request("PUT", "http://h/long")?, &mut source, &hc.sized(5), &later()?, &never), &mut checked)?
    source = pieces("hello")?
    done(checked.send_from(hc.request("PUT", "http://h/short")?, &mut source, &hc.sized(8), &later()?, &never), &mut checked)?
    source = pieces("bad")?
    source.fail = True
    done(checked.send_from(hc.request("PUT", "http://h/fails")?, &mut source, &hc.chunked(), &later()?, &never), &mut checked)?
    stats(&checked)?
    print("--- 100-continue")?
    mut quick = hc.config()?
    quick.continue_timeout = std.time.milliseconds(100u32)
    mut expect = hc.sized(5)
    expect.expect_continue = True
    e = script(["HTTP/1.1 100 Continue\r\n\r\n", ok("e")?]?, True)?
    mut waits = scripted([e]?, copy(quick).unwrap())?
    source = pieces("hello")?
    done(waits.send_from(hc.request("PUT", "http://h/continue")?, &mut source, &expect, &later()?, &never), &mut waits)?
    stats(&waits)?
    print("--- a final answer first leaves the body unsent")?
    f = script(["HTTP/1.1 417 Expectation Failed\r\nContent-Length: 0\r\n\r\n"]?, True)?
    mut refused = scripted([f]?, copy(quick).unwrap())?
    source = pieces("hello")?
    done(refused.send_from(hc.request("PUT", "http://h/refused")?, &mut source, &expect, &later()?, &never), &mut refused)?
    print(source.at)?
    stats(&refused)?
    print("--- a silent server gets the body after continue_timeout")?
    g = script(["", ok("g")?]?, True)?
    mut silent = scripted([g]?, copy(quick).unwrap())?
    source = pieces("hello")?
    done(silent.send_from(hc.request("PUT", "http://h/silent")?, &mut source, &expect, &later()?, &never), &mut silent)?
    stats(&silent)?
    print("--- replayed only while the body is unread")?
    h = script([ok("h")?, ""]?, False)?
    i = script([ok("i")?]?, True)?
    mut replays = scripted([h, i]?, hc.config()?)?
    show(replays.get("http://h/1", &later()?, &never))?
    source = pieces("hello")?
    done(replays.send_from(hc.request("PUT", "http://h/2")?, &mut source, &expect, &later()?, &never), &mut replays)?
    stats(&replays)?
    j = script([ok("j")?, ""]?, False)?
    k = script([ok("k")?]?, True)?
    mut once = scripted([j, k]?, hc.config()?)?
    show(once.get("http://h/1", &later()?, &never))?
    source = pieces("hello")?
    done(once.send_from(hc.request("PUT", "http://h/2")?, &mut source, &hc.sized(5), &later()?, &never), &mut once)?
    stats(&once)?
    print("--- redirects that need the body again are returned")?
    l = script(["HTTP/1.1 307 Temporary Redirect\r\nLocation: /again\r\nContent-Length: 0\r\n\r\n", "HTTP/1.1 303 See Other\r\nLocation: /done\r\nContent-Length: 0\r\n\r\n", ok("l")?]?, True)?
    mut moves = scripted([l]?, hc.config()?)?
    source = pieces("hello")?
    done(moves.send_from(hc.request("PUT", "http://h/a")?, &mut source, &hc.sized(5), &later()?, &never), &mut moves)?
    source = pieces("hello")?
    done(moves.send_from(hc.request("POST", "http://h/b")?, &mut source, &hc.sized(5), &later()?, &never), &mut moves)?
    print("--- an in-memory body is refused")?
    mut both = hc.request("PUT", "http://h/c")?
    both.body = std.bytes.from_text("x")?
    source = pieces("hello")?
    done(moves.send_from(both, &mut source, &hc.chunked(), &later()?, &never), &mut moves)?
    source = pieces("hello")?
    done(moves.send_from(hc.request("PUT", "http://h/c")?, &mut source, &hc.sized(-1), &later()?, &never), &mut moves)?
    stats(&moves)?
    Ok(())
"##,
        r##"[connect http://h:80]
PUT /sized HTTP/1.1|Host: h|Content-Length: 11||hello world
=> 200 http://h/sized redirects=0 body=s1
POST /chunked HTTP/1.1|Host: h|Transfer-Encoding: chunked||4|hell|4|o wo|3|rld|0||
=> 200 http://h/chunked redirects=0 body=c1
opened=1 reused=1 retried=0 redirected=0 discarded=0 idle=1
--- the length must match
[connect http://h:80]
PUT /long HTTP/1.1|Host: h|Content-Length: 5||hell
=> error
std.http.client.ClientError.WrongBodyLength
[connect http://h:80]
PUT /short HTTP/1.1|Host: h|Content-Length: 8||hello
=> error
std.http.client.ClientError.WrongBodyLength
[connect http://h:80]
PUT /fails HTTP/1.1|Host: h|Transfer-Encoding: chunked||3|bad|
=> error
std.http.client.ClientError.Body(std.io.StreamError.Io(IoError.Other))
opened=3 reused=0 retried=0 redirected=0 discarded=0 idle=0
--- 100-continue
[connect http://h:80]
PUT /continue HTTP/1.1|Host: h|Content-Length: 5|Expect: 100-continue||hello
=> 200 http://h/continue redirects=0 body=e
opened=1 reused=0 retried=0 redirected=0 discarded=0 idle=1
--- a final answer first leaves the body unsent
[connect http://h:80]
PUT /refused HTTP/1.1|Host: h|Content-Length: 5|Expect: 100-continue||
=> 417 http://h/refused redirects=0 body=
0
opened=1 reused=0 retried=0 redirected=0 discarded=0 idle=0
--- a silent server gets the body after continue_timeout
[connect http://h:80]
PUT /silent HTTP/1.1|Host: h|Content-Length: 5|Expect: 100-continue||hello
=> 200 http://h/silent redirects=0 body=g
opened=1 reused=0 retried=0 redirected=0 discarded=0 idle=1
--- replayed only while the body is unread
[connect http://h:80]
GET /1 HTTP/1.1|Host: h||
=> 200 http://h/1 redirects=0 body=h
PUT /2 HTTP/1.1|Host: h|Content-Length: 5|Expect: 100-continue||[connect http://h:80]
PUT /2 HTTP/1.1|Host: h|Content-Length: 5|Expect: 100-continue||
=> 200 http://h/2 redirects=0 body=i
opened=2 reused=1 retried=1 redirected=0 discarded=0 idle=0
[connect http://h:80]
GET /1 HTTP/1.1|Host: h||
=> 200 http://h/1 redirects=0 body=j
PUT /2 HTTP/1.1|Host: h|Content-Length: 5||hello
=> error
std.http.client.ClientError.Http(std.http.HttpError.Truncated)
opened=1 reused=1 retried=0 redirected=0 discarded=0 idle=0
--- redirects that need the body again are returned
[connect http://h:80]
PUT /a HTTP/1.1|Host: h|Content-Length: 5||hello
=> 307 http://h/a redirects=0 body=
POST /b HTTP/1.1|Host: h|Content-Length: 5||helloGET /done HTTP/1.1|Host: h||
=> 200 http://h/done redirects=1 body=l
--- an in-memory body is refused

=> error
std.http.client.ClientError.InvalidRequest

=> error
std.http.client.ClientError.InvalidRequest
opened=1 reused=2 retried=0 redirected=1 discarded=0 idle=1"##,
    );
}

/// Short bodies, body limits, interim and `101` responses, chunked bodies read
/// in pieces, and HEAD.
#[test]
fn response_framing_faults_and_interim_responses() {
    runs(
        SCRIPTED,
        r##"
def main() -> Result[(), Failure]:
    never = CancellationToken()?
    a = script(["HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nabc"]?, False)?
    mut client = scripted([a]?, hc.config()?)?
    show(client.get("http://h/short", &later()?, &never))?
    print("--- interim responses are skipped")?
    b = script(["HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 103 Early Hints\r\nLink: x\r\n\r\n".concat(&ok("b")?)?]?, True)?
    mut second = scripted([b]?, hc.config()?)?
    show(second.get("http://h/1", &later()?, &never))?
    c = script(["HTTP/1.1 101 Switching Protocols\r\nUpgrade: x\r\n\r\n"]?, True)?
    mut third = scripted([c]?, hc.config()?)?
    show(third.get("http://h/1", &later()?, &never))?
    print("--- body limit, chunked body, streamed read")?
    d = script([ok("0123456789")?]?, True)?
    mut config = hc.config()?
    config.limits.max_body = 4
    mut fourth = scripted([d]?, config)?
    show(fourth.get("http://h/1", &later()?, &never))?
    e = script(["HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n", ok("f")?]?, True)?
    mut fifth = scripted([e]?, hc.config()?)?
    mut response = fifth.send(hc.request("GET", "http://h/1")?, &later()?, &never)?
    mut buffer = std.bytes.empty()?
    buffer.resize(2, 0u8)?
    while True:
        count = response.read_body(&mut buffer, 0, 2)?
        if count == 0:
            break
        print(buffer.slice(0, count)?.to_text()?)?
    fifth.finish(response)
    show(fifth.get("http://h/2", &later()?, &never))?
    stats(&fifth)?
    print("--- HEAD has no body")?
    g = script(["HTTP/1.1 200 OK\r\nContent-Length: 99\r\n\r\n", ok("g")?]?, True)?
    mut sixth = scripted([g]?, hc.config()?)?
    show(sixth.fetch(hc.request("HEAD", "http://h/1")?, &later()?, &never))?
    show(sixth.get("http://h/2", &later()?, &never))?
    stats(&sixth)?
    Ok(())
"##,
        r##"[connect http://h:80]
GET /short HTTP/1.1|Host: h||
=> error
std.http.client.ClientError.Http(std.http.HttpError.Truncated)
--- interim responses are skipped
[connect http://h:80]
GET /1 HTTP/1.1|Host: h||
=> 200 http://h/1 redirects=0 body=b
[connect http://h:80]
GET /1 HTTP/1.1|Host: h||
=> error
std.http.client.ClientError.UnexpectedInterim
--- body limit, chunked body, streamed read
[connect http://h:80]
GET /1 HTTP/1.1|Host: h||
=> error
std.http.client.ClientError.Http(std.http.HttpError.BodyTooLarge)
[connect http://h:80]
GET /1 HTTP/1.1|Host: h||ab
c
de
GET /2 HTTP/1.1|Host: h||
=> 200 http://h/2 redirects=0 body=f
opened=1 reused=1 retried=0 redirected=0 discarded=0 idle=1
--- HEAD has no body
[connect http://h:80]
HEAD /1 HTTP/1.1|Host: h||
=> 200 http://h/1 redirects=0 body=
GET /2 HTTP/1.1|Host: h||
=> 200 http://h/2 redirects=0 body=g
opened=1 reused=1 retried=0 redirected=0 discarded=0 idle=1"##,
    );
}

/// A failed body read closes the connection and drops the body, so later
/// reads report `Broken` and the connection is not pooled.
#[test]
fn reads_after_a_failed_body_read_report_broken() {
    runs(
        SCRIPTED,
        r##"
def main() -> Result[(), Failure]:
    never = CancellationToken()?
    a = script(["HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nabc", ok("b")?]?, True)?
    mut client = scripted([a]?, hc.config()?)?
    mut response = client.send(hc.request("GET", "http://h/1")?, &later()?, &never)?
    mut buffer = std.bytes.empty()?
    buffer.resize(8, 0u8)?
    mut body = std.bytes.empty()?
    while True:
        match response.read_body(&mut buffer, 0, 8):
            case Ok(count):
                if count == 0:
                    break
                print(buffer.slice(0, count)?.to_text()?)?
            case Err(error):
                print(error)?
                break
    match response.read_body(&mut buffer, 0, 8):
        case Ok(_):
            print("unexpected read")?
        case Err(error):
            print(error)?
    match response.read_all(&mut body):
        case Ok(_):
            print("unexpected read")?
        case Err(error):
            print(error)?
    client.finish(response)
    stats(&client)?
    Ok(())
"##,
        r##"[connect http://h:80]
GET /1 HTTP/1.1|Host: h||abc
std.http.client.ClientError.Net(std.net.NetError.TimedOut)
std.http.client.ClientError.Http(std.http.HttpError.Broken)
std.http.client.ClientError.Http(std.http.HttpError.Broken)
opened=1 reused=0 retried=0 redirected=0 discarded=0 idle=0"##,
    );
}

/// Relative, scheme-relative and absolute locations; 302 after POST and 303
/// become GET without a body; 307 repeats the body; credentials and listed
/// fields go no further once the origin changes; loops stop at the limit.
#[test]
fn redirects_rewrite_methods_and_drop_credentials_across_origins() {
    runs(
        SCRIPTED,
        r##"
def moved(status: i64, location: &str) -> Result[str, Failure]:
    Ok("HTTP/1.1 ".concat(&str.from(status)?)?.concat(" X\r\nLocation: ")?.concat(location)?.concat("\r\nContent-Length: 3\r\n\r\nold")?)

def main() -> Result[(), Failure]:
    never = CancellationToken()?
    mut config = hc.config()?
    config.sensitive.append("X-Api-Key")?
    a = script([moved(302, "../c/./d?q=1#frag")?, moved(307, "//other:8080/e")?, ok("done")?]?, True)?
    b = script([moved(303, "http://h/back")?]?, True)?
    mut client = scripted([a, b]?, config)?
    mut post = hc.request("POST", "http://h/a/b")?
    post.add("Authorization", "Bearer s")?
    post.add("Cookie", "id=1")?
    post.add("X-Api-Key", "k")?
    post.add("Content-Type", "text/plain")?
    post.add("Accept", "*/*")?
    post.body = std.bytes.from_text("body")?
    show(client.fetch(post, &later()?, &never))?
    stats(&client)?
    print("--- 307 repeats the method and body")?
    c = script([moved(307, "/x")?, ok("y")?]?, True)?
    mut second = scripted([c]?, hc.config()?)?
    mut put = hc.request("PUT", "http://h/a")?
    put.add("Authorization", "Bearer s")?
    put.body = std.bytes.from_text("kept")?
    show(second.fetch(put, &later()?, &never))?
    print("--- a loop stops at the limit")?
    d = script([moved(301, "/a")?, moved(301, "/a")?, moved(301, "/a")?]?, True)?
    mut limited = hc.config()?
    limited.redirects = 2
    mut third = scripted([d]?, limited)?
    show(third.get("http://h/a", &later()?, &never))?
    print("--- redirects 0 returns the redirect")?
    e = script([moved(301, "/a")?]?, True)?
    mut none = hc.config()?
    none.redirects = 0
    mut fourth = scripted([e]?, none)?
    show(fourth.get("http://h/z", &later()?, &never))?
    print("--- other schemes and bad locations are refused")?
    f = script([moved(302, "ftp://h/a")?, moved(302, "http://[bad")?]?, True)?
    mut fifth = scripted([f]?, hc.config()?)?
    show(fifth.get("http://h/z", &later()?, &never))?
    show(fifth.get("http://h/z", &later()?, &never))?
    Ok(())
"##,
        r##"[connect http://h:80]
POST /a/b HTTP/1.1|Host: h|Authorization: Bearer s|Cookie: id=1|X-Api-Key: k|Content-Type: text/plain|Accept: */*|Content-Length: 4||bodyGET /c/d?q=1 HTTP/1.1|Host: h|Authorization: Bearer s|Cookie: id=1|X-Api-Key: k|Accept: */*||[connect http://other:8080]
GET /e HTTP/1.1|Host: other:8080|Accept: */*||GET /back HTTP/1.1|Host: h|Accept: */*||
=> 200 http://h/back redirects=3 body=done
opened=2 reused=2 retried=0 redirected=3 discarded=0 idle=2
--- 307 repeats the method and body
[connect http://h:80]
PUT /a HTTP/1.1|Host: h|Authorization: Bearer s|Content-Length: 4||keptPUT /x HTTP/1.1|Host: h|Authorization: Bearer s|Content-Length: 4||kept
=> 200 http://h/x redirects=1 body=y
--- a loop stops at the limit
[connect http://h:80]
GET /a HTTP/1.1|Host: h||GET /a HTTP/1.1|Host: h||GET /a HTTP/1.1|Host: h||
=> error
std.http.client.ClientError.TooManyRedirects
--- redirects 0 returns the redirect
[connect http://h:80]
GET /z HTTP/1.1|Host: h||
=> 301 http://h/z redirects=0 body=old
--- other schemes and bad locations are refused
[connect http://h:80]
GET /z HTTP/1.1|Host: h||
=> error
std.http.client.ClientError.UnsupportedScheme
GET /z HTTP/1.1|Host: h||
=> error
std.http.client.ClientError.InvalidRedirect"##,
    );
}

/// Requests through an explicit proxy use absolute-form targets on one pooled
/// proxy connection, and only the proxy sees `Proxy-Authorization`.
#[test]
fn a_proxy_gets_absolute_targets_and_its_own_credentials() {
    runs(
        SCRIPTED,
        r##"
def main() -> Result[(), Failure]:
    never = CancellationToken()?
    mut config = hc.config()?
    mut via = hc.proxy("http://proxy.local:3128")?
    via.authorization = Some("Basic cHJveHk=")
    config.proxy = Some(via)
    a = script([ok("p1")?, "HTTP/1.1 302 Found\r\nLocation: http://elsewhere/\r\nContent-Length: 0\r\n\r\n", ok("p3")?]?, True)?
    mut client = scripted([a]?, config)?
    show(client.get("http://h:81/a?b", &later()?, &never))?
    mut secret = hc.request("GET", "http://h:81/redirect")?
    secret.add("Authorization", "Bearer s")?
    show(client.fetch(secret, &later()?, &never))?
    stats(&client)?
    print("--- proxy URLs")?
    match hc.proxy("http://user:pw@proxy.local/"):
        case Ok(_):
            print("accepted")?
        case Err(error):
            print(error)?
    match hc.proxy("http://proxy.local/path"):
        case Ok(_):
            print("accepted")?
        case Err(error):
            print(error)?
    match hc.proxy("https://proxy.local/"):
        case Ok(_):
            print("accepted")?
        case Err(error):
            print(error)?
    Ok(())
"##,
        r##"[connect http://proxy.local:3128]
GET http://h:81/a?b HTTP/1.1|Host: h:81|Proxy-Authorization: Basic cHJveHk=||
=> 200 http://h:81/a?b redirects=0 body=p1
GET http://h:81/redirect HTTP/1.1|Host: h:81|Authorization: Bearer s|Proxy-Authorization: Basic cHJveHk=||GET http://elsewhere/ HTTP/1.1|Host: elsewhere|Proxy-Authorization: Basic cHJveHk=||
=> 200 http://elsewhere/ redirects=1 body=p3
opened=1 reused=2 retried=0 redirected=1 discarded=0 idle=1
--- proxy URLs
std.http.client.ClientError.InvalidUrl
std.http.client.ClientError.InvalidUrl
std.http.client.ClientError.InvalidUrl"##,
    );
}

/// Client-managed fields, CONNECT, bad methods, credentials in URLs, other
/// schemes and bad config are refused without opening a connection.
#[test]
fn invalid_requests_fail_before_connecting() {
    runs(
        SCRIPTED,
        r##"
def main() -> Result[(), Failure]:
    never = CancellationToken()?
    mut client = scripted([]?, hc.config()?)?
    mut host = hc.request("GET", "http://h/")?
    host.add("host", "evil")?
    show(client.fetch(host, &later()?, &never))?
    mut length = hc.request("POST", "http://h/")?
    length.add("Transfer-Encoding", "chunked")?
    show(client.fetch(length, &later()?, &never))?
    mut proxy = hc.request("GET", "http://h/")?
    proxy.add("Proxy-Authorization", "x")?
    show(client.fetch(proxy, &later()?, &never))?
    show(client.fetch(hc.request("CONNECT", "http://h/")?, &later()?, &never))?
    show(client.fetch(hc.request("GE T", "http://h/")?, &later()?, &never))?
    show(client.get("http://user:pw@h/", &later()?, &never))?
    show(client.get("ftp://h/", &later()?, &never))?
    show(client.get("h/", &later()?, &never))?
    show(client.get("http://h:0/", &later()?, &never))?
    stats(&client)?
    mut config = hc.config()?
    config.redirects = -1
    match hc.client(config):
        case Ok(_):
            print("accepted")?
        case Err(error):
            print(error)?
    mut tcp = hc.client(hc.config()?)?
    show(tcp.get("https://127.0.0.1:1/", &later()?, &never))?
    Ok(())
"##,
        r##"
=> error
std.http.client.ClientError.InvalidRequest

=> error
std.http.client.ClientError.InvalidRequest

=> error
std.http.client.ClientError.InvalidRequest

=> error
std.http.client.ClientError.InvalidRequest

=> error
std.http.client.ClientError.InvalidRequest

=> error
std.http.client.ClientError.InvalidUrl

=> error
std.http.client.ClientError.UnsupportedScheme

=> error
std.http.client.ClientError.Url(std.url.UrlError.MissingScheme)

=> error
std.http.client.ClientError.InvalidUrl
opened=0 reused=0 retried=0 redirected=0 discarded=0 idle=0
std.http.client.ClientError.InvalidConfig

=> error
std.http.client.ClientError.UnsupportedScheme"##,
    );
}

/// Against `std.http.server` on loopback: reuse, a redirect and a POST body;
/// then a server that never answers is left by the head timeout, the caller's
/// deadline and cancellation.
#[test]
fn loopback_exchanges_deadlines_and_cancellation() {
    runs(
        "",
        r##"import std.http.client as hc
import std.bytes
import std.http
import std.http.server
import std.net
import std.time
from std.http.server import Exchange
from std.net import TcpListener, TcpStream
from std.sync import CancellationToken

pub enum Route:
    Home
    Old
    Echo

pub class Site:
    pub def handle(self, route: &Route, exchange: &mut Exchange[TcpStream]) -> Result[(), Failure]:
        match route:
            case Route.Home:
                exchange.send_text(200, "home\n")?
            case Route.Old:
                mut head = std.http.response(302, "Found")?
                head.add("Location", "/")?
                exchange.respond(head, &std.bytes.from_text("moved")?)?
            case Route.Echo:
                mut body = std.bytes.empty()?
                exchange.read_all(&mut body)?
                exchange.send(200, "application/octet-stream", &body)?
        Ok(())

def later() -> Result[std.time.Deadline, Failure]:
    Ok(std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(5u32))?)

def visit(port: u16, stop: CancellationToken) -> Result[(), Failure]:
    never = CancellationToken()?
    mut client = hc.client(hc.config()?)?
    base = "http://127.0.0.1:".concat(&str.from(i64(port))?)?
    home = client.get(&base.concat("/")?, &later()?, &never)?
    print(home.status, home.body.to_text()?.strip()?)?
    moved = client.get(&base.concat("/old")?, &later()?, &never)?
    print(moved.status, moved.redirects, moved.url == base.concat("/")?)?
    mut post = hc.request("POST", &base.concat("/echo")?)?
    post.body = std.bytes.from_text("payload")?
    echoed = client.fetch(post, &later()?, &never)?
    print(echoed.body.to_text()?)?
    stats = client.stats()
    print(stats.opened, stats.reused, stats.redirected)?
    stop.cancel()
    Ok(())

# Accepts connections and never answers until `stop` is cancelled.
def silent(listener: TcpListener, stop: CancellationToken) -> Result[(), Failure]:
    mut held = list[TcpStream].new()?
    while not stop.is_cancelled():
        match listener.accept(&later()?, &stop):
            case Ok(stream):
                held.append(stream)?
            case Err(_):
                pass
    Ok(())

def canceller(token: CancellationToken) -> ():
    std.time.sleep(&std.time.milliseconds(100u32))
    token.cancel()

def main() -> Result[(), Failure]:
    mut routes = std.http.server.router[Route]()?
    routes.add("GET", "/", Route.Home)?
    routes.add("GET", "/old", Route.Old)?
    routes.add("POST", "/echo", Route.Echo)?
    listener = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    port = listener.local_address()?.port
    stop = CancellationToken()?
    with spawn(visit, port, stop.share())? as task:
        report = std.http.server.serve(listener, &routes, &Site(), &std.http.server.config(), &stop)?
        print(report.connections, report.requests)?
    # A server that never answers: the head timeout, the caller's deadline
    # and cancellation each end the wait.
    quiet = std.net.listen(&std.net.loopback_v4(0u16), 16)?
    url = "http://127.0.0.1:".concat(&str.from(i64(quiet.local_address()?.port))?)?.concat("/")?
    halt = CancellationToken()?
    with spawn(silent, quiet, halt.share())? as server:
        never = CancellationToken()?
        mut config = hc.config()?
        config.head_timeout = std.time.milliseconds(100u32)
        mut client = hc.client(config)?
        start = std.time.monotonic_now()
        match client.get(&url, &later()?, &never):
            case Ok(_):
                print("answered")?
            case Err(error):
                print(error)?
        short = std.time.deadline_after(&std.time.monotonic_now(), &std.time.milliseconds(100u32))?
        match hc.client(hc.config()?)?.get(&url, &short, &never):
            case Ok(_):
                print("answered")?
            case Err(error):
                print(error)?
        cancel = CancellationToken()?
        with spawn(canceller, cancel.share())? as timer:
            match hc.client(hc.config()?)?.get(&url, &later()?, &cancel):
                case Ok(_):
                    print("answered")?
                case Err(error):
                    print(error)?
        waited = std.time.monotonic_now().saturating_duration_since(&start).as_milliseconds()
        print(waited >= 300 and waited < 3000)?
        halt.cancel()
    Ok(())
"##,
        r##"200 home
200 1 True
payload
1 3 1
1 4
std.http.client.ClientError.Net(std.net.NetError.TimedOut)
std.http.client.ClientError.Net(std.net.NetError.TimedOut)
std.http.client.ClientError.Net(std.net.NetError.Cancelled)
True"##,
    );
}

/// Allocation failure at any point is reported as `Allocation`; the program
/// never aborts, and once allocation succeeds the exchange completes.
#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_is_reported() {
    let result = output(&format!(
        "{SCRIPTED}{}",
        r##"
def quiet(replies: list[str]) -> Result[Script, Failure]:
    mut made = script(replies, True)?
    made.echo = False
    Ok(made)

def main() -> Result[(), Failure]:
    never = CancellationToken()?
    for attempt in range(300):
        first = quiet(["HTTP/1.1 307 Temporary Redirect\r\nLocation: /next/page-beyond-inline\r\nContent-Length: 0\r\n\r\n", "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nX-Field-Beyond-Inline: a-value-beyond-inline\r\n\r\n4\r\nbody\r\n0\r\n\r\n"]?)?
        # A connection closed after a failed allocation is replaced by this one.
        spare = quiet(["HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nbody"]?)?
        mut client = hc.connector[Script, Scripts](Scripts([first, spare]?, False), hc.config()?)?
        mut put = hc.request("PUT", "http://host-beyond-inline/first/page")?
        put.add("Authorization", "Bearer a-token-beyond-inline")?
        put.body = std.bytes.from_text("payload-beyond-inline")?
        deadline = later()?
        marker = "__test_fail_allocations_after_".concat(str.from(attempt)?)?.concat("__")?
        print(marker)?
        result = client.fetch(put, &deadline, &never)
        print("__test_restore_allocations__")?
        match result:
            case Ok(fetched):
                print(fetched.body.to_text()?)?
            case Err(ClientError.Allocation(_)):
                print("allocation")?
            case Err(error):
                print(error)?
    Ok(())
"##
    ));
    let lines: Vec<&str> = result.lines().collect();
    assert_eq!(lines.len(), 300);
    assert!(
        lines
            .iter()
            .all(|line| ["allocation", "body"].contains(line)),
        "{lines:?}"
    );
    assert_eq!(lines[0], "allocation");
    assert!(lines[280..].iter().all(|line| *line == "body"), "{lines:?}");
}

/// `fetch` keeps the final head whole (version, reason and every field), the
/// final URL and the redirect count, and pools the connection behind a fully
/// read body. A body over `max_body` fails the fetch and closes its
/// connection.
#[test]
fn fetch_keeps_the_final_head_and_pools_its_connection() {
    runs(
        SCRIPTED,
        r##"
def head_of(fetched: &hc.Fetched) -> Result[(), Failure]:
    head = &fetched.head
    print(fetched.status, head.status, head.minor, head.reason)?
    for index in range(len(head.headers)):
        print(head.headers[index].name, head.headers[index].value)?
    Ok(())

def main() -> Result[(), Failure]:
    never = CancellationToken()?
    moved = "HTTP/1.1 302 Found\r\nLocation: /final\r\nContent-Length: 0\r\n\r\n"
    final = "HTTP/1.1 203 Translated Here\r\nX-One: 1\r\nX-Two: two\r\nContent-Length: 4\r\n\r\nbody"
    mut first = script([moved, final, ok("next")?]?, True)?
    first.echo = False
    mut client = hc.connector[Script, Scripts](Scripts([first]?, False), hc.config()?)?
    fetched = client.get("http://h/start", &later()?, &never)?
    head_of(&fetched)?
    print(fetched.url, fetched.redirects, fetched.body.to_text()?)?
    stats(&client)?
    again = client.get("http://h/again", &later()?, &never)?
    print(again.body.to_text()?)?
    stats(&client)?
    print("--- a body over max_body closes its connection")?
    mut big = script([ok("too long")?, ok("x")?]?, True)?
    big.echo = False
    mut config = hc.config()?
    config.limits.max_body = 3
    mut second = hc.connector[Script, Scripts](Scripts([big]?, False), config)?
    show(second.get("http://h/big", &later()?, &never))?
    stats(&second)?
    Ok(())
"##,
        r##"203 203 1 Translated Here
X-One 1
X-Two two
Content-Length 4
http://h/final 1 body
opened=1 reused=1 retried=0 redirected=1 discarded=0 idle=1
next
opened=1 reused=2 retried=0 redirected=1 discarded=0 idle=1
--- a body over max_body closes its connection

=> error
std.http.client.ClientError.Http(std.http.HttpError.BodyTooLarge)
opened=1 reused=0 retried=0 redirected=0 discarded=0 idle=0"##,
    );
}

/// `fetch` allocates exactly what `send`, an empty buffer, `read_all` and
/// `finish` allocate: under every injected allocation failure both report
/// the same outcome, so no extra allocation (such as a placeholder head)
/// can fail a fetch whose body has been read.
#[cfg(feature = "runtime-checks")]
#[test]
fn fetch_allocates_no_more_than_send_read_all_and_finish() {
    let result = output(&format!(
        "{SCRIPTED}{}",
        r##"
from std.bytes import BytesError

def client() -> Result[hc.Client[Script, Scripts], Failure]:
    mut made = script(["HTTP/1.1 200 OK\r\nX-Field-Beyond-Inline: a-value-beyond-inline\r\nContent-Length: 4\r\n\r\nbody"]?, True)?
    made.echo = False
    Ok(hc.connector[Script, Scripts](Scripts([made]?, False), hc.config()?)?)

def by_hand(client: &mut hc.Client[Script, Scripts], request: Request, deadline: &Deadline, never: &CancellationToken) -> Result[(), ClientError]:
    mut response = client.send(request, deadline, never)?
    mut body = match std.bytes.empty():
        case Ok(made):
            made
        case Err(BytesError.Allocation(cause)):
            return Err(ClientError.Allocation(cause))
        case Err(_):
            return Err(ClientError.Http(HttpError.Broken))
    response.read_all(&mut body)?
    client.finish(response)
    Ok(())

def tag(result: &Result[(), ClientError], idle: i64) -> str:
    match result:
        case Ok(_):
            if idle == 1:
                "ok-pooled"
            else:
                "ok-closed"
        case Err(ClientError.Allocation(_)):
            "allocation"
        case Err(_):
            "other"

def main() -> Result[(), Failure]:
    never = CancellationToken()?
    for attempt in range(200):
        mut one = client()?
        mut two = client()?
        first = hc.request("GET", "http://host-beyond-inline/page")?
        second = hc.request("GET", "http://host-beyond-inline/page")?
        deadline = later()?
        marker = "__test_fail_allocations_after_".concat(str.from(attempt)?)?.concat("__")?
        print(marker)?
        fetched: Result[(), ClientError] = match one.fetch(first, &deadline, &never):
            case Ok(_):
                Ok(())
            case Err(error):
                Err(error)
        print("__test_restore_allocations__")?
        print(marker)?
        manual = by_hand(&mut two, second, &deadline, &never)
        print("__test_restore_allocations__")?
        print(tag(&fetched, one.idle()), tag(&manual, two.idle()))?
    Ok(())
"##
    ));
    let lines: Vec<&str> = result.lines().collect();
    assert_eq!(lines.len(), 200);
    for line in &lines {
        let (fetched, manual) = line.split_once(' ').unwrap();
        assert_eq!(fetched, manual, "{lines:?}");
        assert!(
            ["allocation", "ok-pooled", "ok-closed"].contains(&fetched),
            "{lines:?}"
        );
    }
    assert_eq!(lines[0], "allocation allocation");
    assert_eq!(lines[199], "ok-pooled ok-pooled");
}

/// Issue #135: a request shares its method and URL text instead of copying
/// it, so long ones need no more allocations than short ones, and both
/// outlive their sources.
#[cfg(feature = "runtime-checks")]
#[test]
fn request_shares_its_method_and_url() {
    runs(
        "",
        r##"import std.http.client as hc

def needed(method: &str, url: &str) -> Result[i64, Failure]:
    for budget in range(20):
        marker = "__test_fail_allocations_after_".concat(str.from(budget)?)?.concat("__")?
        print(marker)?
        outcome = hc.request(method, url)
        print("__test_restore_allocations__")?
        if outcome.is_ok():
            return Ok(budget)
    Ok(-1)

def main() -> Result[(), Failure]:
    print(needed("GET", "http://h/")?)?
    mut url = ""
    mut method = ""
    if True:
        given = "http://a-host-name-beyond-inline.example/".concat("a-path-beyond-inline")?
        verb = "PROPFIND".concat("-BEYOND-INLINE")?
        print(needed(&verb, &given)?)?
        made = hc.request(&verb, &given)?
        url = made.url
        method = made.method
    print(method)?
    print(url)?
    Ok(())
"##,
        "2\n2\nPROPFIND-BEYOND-INLINE\nhttp://a-host-name-beyond-inline.example/a-path-beyond-inline",
    );
}
