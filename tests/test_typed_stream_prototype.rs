use std::process::Command;

const FIXTURES: &str = r#"
import std.bytes
from std.bytes import Bytes
from streams import StepError, Partial, ReadFailure, WriteFailure, CopyFailure, read_exact, write_all, copy_all
from http_body import FixedBody, FramingError, fixed_body
pub enum Transport:
    Tls(i64, str)
    Cancelled
    Allocation(AllocError)
pub enum Storage:
    Full(i64)
def scratch() -> Result[Bytes, AllocError | std.bytes.BytesError]:
    mut buffer = std.bytes.with_capacity(4)?
    buffer.resize(4, 0u8)?
    Ok(buffer)
pub class Source:
    mode: i64
    calls: i64
    pub def read(self: &mut Source, buffer: &mut Bytes, start: i64, stop: i64) -> Result[i64, StepError[Transport]]:
        self.calls += 1
        if self.calls == 1:
            return Err(StepError[Transport].Interrupted)
        if self.calls == 2:
            mut count = stop - start
            if count > 2:
                count = 2
            mut at = start
            while at < start + count:
                buffer.set(at, 65u8).unwrap()
                at += 1
            return Ok(count)
        if self.mode == 1:
            return Err(StepError[Transport].Source(Transport.Tls(47, "handshake")))
        if self.mode == 2:
            return Err(StepError[Transport].Source(Transport.Cancelled))
        if self.mode == 3:
            return Ok(stop - start + 1)
        if self.mode == 4:
            return Ok(-1)
        if self.mode == 5:
            return Err(StepError[Transport].Source(Transport.Allocation(AllocError.OutOfMemory)))
        Ok(0)
pub class Sink:
    mode: i64
    calls: i64
    delivered: i64
    pub def write(self: &mut Sink, buffer: &Bytes, start: i64, stop: i64) -> Result[i64, StepError[Storage]]:
        self.calls += 1
        if self.calls == 1:
            return Err(StepError[Storage].Interrupted)
        if self.calls > 2 and self.mode == 1:
            return Err(StepError[Storage].Source(Storage.Full(28)))
        if self.mode == 2:
            return Ok(0)
        if self.mode == 3:
            return Ok(-1)
        if self.mode == 4:
            return Ok(stop - start + 1)
        self.delivered += 1
        Ok(1)
"#;

fn fixture(source: &str) -> tempfile::TempDir {
    let workspace = tempfile::tempdir().unwrap();
    for module in ["streams", "http_body"] {
        std::fs::copy(
            format!(
                "{}/examples/error_composition/{module}.plenty",
                env!("CARGO_MANIFEST_DIR")
            ),
            workspace.path().join(format!("{module}.plenty")),
        )
        .unwrap();
    }
    let file = workspace.path().join("main.plenty");
    std::fs::write(&file, source).unwrap();
    workspace
}

fn execute(body: &str, expected: &str) {
    execute_modules(body, expected, false);
    execute_modules(body, expected, true);
}

fn execute_modules(body: &str, expected: &str, standard: bool) {
    let source = format!("{FIXTURES}\n{body}");
    let source = if standard {
        source
            .replace("from streams import", "from std.io.typed import")
            .replace("from http_body import", "from std.http.typed import")
    } else {
        source
    };
    let workspace = fixture(&source);
    let file = workspace.path().join("main.plenty");
    let binary = workspace.path().join("program");
    plenty::compile_file_to_executable(&file, &binary, None).unwrap();
    let output = Command::new(binary).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let visible: String = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .map(|line| format!("{line}\n"))
        .collect();
    assert_eq!(visible, expected);
}

#[test]
fn incompatible_protocol_error_is_diagnosed() {
    let source = format!(
        "{FIXTURES}\ndef main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:\n    mut source = Source(0, 0)\n    mut buffer = scratch()?\n    drop(read_exact[Storage, Source](&mut source, &mut buffer, 0, 4))\n    Ok(())\n"
    );
    let workspace = fixture(&source);
    let error = plenty::check_file(&workspace.path().join("main.plenty"), None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("signature"), "{error}");
}

#[test]
fn production_bridges_copy_fixed_and_close_delimited_bodies() {
    execute_modules(
        r#"
import std.io
import std.io.typed
from std.http.typed import close_body
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | std.io.StreamError | StepError[FramingError[std.io.StreamError]] | FramingError[std.io.StreamError] | CopyFailure[FramingError[std.io.StreamError], std.io.StreamError]]:
    mut data = scratch()?
    data.set(0, 65u8)?
    data.set(1, 66u8)?
    mut buffer = scratch()?
    mut fixed = fixed_body(std.io.typed.reader(std.io.cursor(data)), 2)?
    mut destination = std.io.typed.writer(std.io.cursor(std.bytes.with_capacity(4)?))
    print(copy_all(&mut fixed, &mut destination, &mut buffer)?)?
    mut cursor = std.io.typed.unwrap_writer(destination)
    print(cursor.position())?
    cursor.seek(std.io.SeekFrom.Start(0))?
    mut closed = close_body(std.io.typed.reader(cursor))
    mut output = std.io.typed.writer(std.io.cursor(std.bytes.with_capacity(4)?))
    print(copy_all(&mut closed, &mut output, &mut buffer)?)?
    print(closed.read(&mut buffer, 0, 4)?)?
    mut output_cursor = std.io.typed.unwrap_writer(output)
    output_cursor.seek(std.io.SeekFrom.Start(0))?
    print(output_cursor.read(&mut buffer, 0, 4)?)?
    print(buffer.get(0).unwrap())?
    print(buffer.get(1).unwrap())?
    Ok(())
"#,
        "2\n2\n2\n0\n2\n65\n66\n",
        true,
    );
}

#[test]
fn close_body_preserves_causes_progress_and_terminal_state() {
    execute_modules(
        r#"
from std.http.typed import close_body
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | StepError[FramingError[Transport]]]:
    mut buffer = scratch()?
    mut body = close_body(Source(1, 0))
    match body.read(&mut buffer, -1, 0):
        case Err(StepError.Source(FramingError.OutOfRange)):
            print("range")?
        case _:
            print("wrong")?
    print(body.read(&mut buffer, 0, 0)?)?
    mut sink = Sink(0, 0, 0)
    match copy_all(&mut body, &mut sink, &mut buffer):
        case Err(CopyFailure.Read(count, ReadFailure.Source(FramingError.Transport(Transport.Tls(code, message))))):
            print(count)?
            print(code)?
            print(message)?
        case _:
            print("wrong")?
    match body.read(&mut buffer, 0, 4):
        case Err(StepError.Source(FramingError.Broken)):
            print("broken")?
        case _:
            print("wrong")?
    mut invalid = close_body(Source(3, 2))
    match invalid.read(&mut buffer, 0, 4):
        case Err(StepError.Source(FramingError.InvalidProgress)):
            print("progress")?
        case _:
            print("wrong")?
    mut cancelled = close_body(Source(2, 2))
    match cancelled.read(&mut buffer, 0, 4):
        case Err(StepError.Source(FramingError.Transport(Transport.Cancelled))):
            print("cancelled")?
        case _:
            print("wrong")?
    Ok(())
"#,
        "range\n0\n2\n47\nhandshake\nbroken\nprogress\ncancelled\n",
        true,
    );
}

#[test]
fn close_body_empty_reads_do_not_consume_input_and_eof_is_sticky() {
    execute_modules(
        r#"
from std.http.typed import close_body
pub class Ends:
    calls: i64
    pub def read(self: &mut Ends, buffer: &mut Bytes, start: i64, stop: i64) -> Result[i64, StepError[Transport]]:
        self.calls += 1
        print(self.calls).unwrap()
        if self.calls == 1:
            return Ok(0)
        Err(StepError[Transport].Source(Transport.Cancelled))
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | StepError[FramingError[Transport]]]:
    mut body = close_body(Ends(0))
    mut buffer = scratch()?
    print(body.read(&mut buffer, 2, 2)?)?
    print(body.read(&mut buffer, 0, 4)?)?
    print(body.read(&mut buffer, 0, 4)?)?
    Ok(())
"#,
        "0\n1\n0\n0\n",
        true,
    );
}

const CHUNK_SOURCE: &str = r#"
from std.http.typed import chunked_body
from std.http import Limits, HttpError
pub class Wire:
    data: Bytes
    position: i64
    interrupted: bool
    fail_at: i64
    mode: i64
    pub def read(self: &mut Wire, buffer: &mut Bytes, start: i64, stop: i64) -> Result[i64, StepError[Transport]]:
        if not self.interrupted:
            self.interrupted = True
            return Err(StepError[Transport].Interrupted)
        self.interrupted = False
        if self.position == self.fail_at:
            if self.mode == 1:
                return Err(StepError[Transport].Source(Transport.Tls(47, "body")))
            if self.mode == 2:
                return Err(StepError[Transport].Source(Transport.Cancelled))
            if self.mode == 3:
                return Ok(-1)
            if self.mode == 4:
                return Ok(stop - start + 1)
            return Err(StepError[Transport].Source(Transport.Allocation(AllocError.OutOfMemory)))
        if self.position == self.data.len():
            return Ok(0)
        code = self.data[self.position]
        if code == 64u8:
            print("overread").unwrap()
        buffer.set(start, code).unwrap()
        self.position += 1
        Ok(1)
def wire(text: str) -> Result[Wire, std.bytes.BytesError]:
    Ok(Wire(std.bytes.from_text(text)?, 0, False, -1, 0))
def reason(error: FramingError[Transport]) -> str:
    match error:
        case FramingError.Truncated:
            "truncated"
        case FramingError.InvalidProgress:
            "progress"
        case FramingError.Protocol(HttpError.InvalidChunk):
            "chunk"
        case FramingError.Protocol(HttpError.BareLineFeed):
            "lf"
        case FramingError.Protocol(HttpError.InvalidHeader):
            "header"
        case FramingError.Protocol(HttpError.LineTooLong):
            "line"
        case FramingError.Protocol(HttpError.HeadTooLarge):
            "head"
        case FramingError.Protocol(HttpError.TooManyHeaders):
            "headers"
        case FramingError.Protocol(HttpError.BodyTooLarge):
            "body"
        case FramingError.Transport(Transport.Cancelled):
            "cancelled"
        case FramingError.Transport(Transport.Allocation(AllocError.OutOfMemory)):
            "allocation"
        case _:
            "unexpected"
"#;

#[test]
fn parsed_heads_select_typed_bodies_and_recover_buffered_next_message() {
    execute_modules(
        r#"
import std.http
import std.http.typed
import std.io
import std.io.typed
from std.http import HttpError
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | std.io.StreamError | HttpError | FramingError[std.io.StreamError] | StepError[FramingError[std.io.StreamError]]]:
    data = std.bytes.from_text("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nABHTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1\r\nC\r\n0\r\nX: yes\r\n\r\nHTTP/1.1 204 No Content\r\n\r\n")?
    mut conn = std.io.buffered_reader(std.io.cursor(data), 256)?
    limits = std.http.limits()
    head = std.http.read_response(&mut conn, &limits)?.unwrap()
    mut body = std.http.typed.response_body(std.io.typed.reader(conn), &head, "GET", True, &limits)?
    print(body.is_done())?
    match body.finish():
        case Err(FramingError.Protocol(HttpError.Unfinished)):
            print("unfinished")?
        case _:
            print("wrong")?
    mut buffer = scratch()?
    print(body.read(&mut buffer, 0, 4)?, body.total(), body.is_done())?
    print(buffer.to_text_range(0, 2)?)?
    mut next = std.io.typed.unwrap_reader(body.finish()?)
    match body.finish():
        case Err(FramingError.Broken):
            print("released")?
        case _:
            print("wrong")?
    match body.read(&mut buffer, 0, 4):
        case Err(StepError.Source(FramingError.Broken)):
            print("broken")?
        case _:
            print("wrong")?
    second = std.http.read_response(&mut next, &limits)?.unwrap()
    mut chunks = std.http.typed.response_body(std.io.typed.reader(next), &second, "GET", True, &limits)?
    print(chunks.read(&mut buffer, 0, 4)?)?
    print(buffer.to_text_range(0, 1)?)?
    print(chunks.is_done())?
    print(chunks.read(&mut buffer, 0, 4)?, chunks.total(), chunks.is_done())?
    print(chunks.trailers()[0].name, chunks.trailers()[0].value)?
    mut last = std.io.typed.unwrap_reader(chunks.finish()?)
    third = std.http.read_response(&mut last, &limits)?.unwrap()
    print(third.status)?
    mut empty = std.http.typed.response_body(std.io.typed.reader(last), &third, "GET", True, &limits)?
    print(empty.is_done(), empty.total(), len(empty.trailers()))?
    drop(empty.finish()?)
    Ok(())
"#,
        "False\nunfinished\n2 2 True\nAB\nreleased\nbroken\n1\nC\nFalse\n0 1 True\nX yes\n204\nTrue 0 0\n",
        true,
    );
}

#[test]
fn typed_heads_share_grammar_preserve_prefetch_and_recover_messages() {
    execute_modules(
        r#"
import std.io
import std.io.typed
import std.http
import std.http.typed
def main() -> Result[(), IoError]:
    bytes = std.bytes.from_text("\r\nGET /one HTTP/1.1\r\nHost: example\r\nContent-Length: 2\r\n\r\nABGET /two HTTP/1.0\r\nHost: example\r\n\r\n").unwrap()
    mut conn = std.io.typed.buffered_reader(std.io.typed.reader(std.io.cursor(bytes)), 256).unwrap()
    limits = std.http.limits()
    first = std.http.typed.read_request(&mut conn, &limits).unwrap().unwrap()
    print(first.method, first.target, first.minor, len(first.headers), conn.buffered() > 2)?
    mut body = std.http.typed.request_body(conn, &first, &limits).unwrap()
    mut buffer = scratch().unwrap()
    print(body.read(&mut buffer, 0, 4).unwrap(), body.is_done())?
    print(buffer.to_text_range(0, 2).unwrap())?
    mut next = body.finish().unwrap()
    second = std.http.typed.read_request(&mut next, &limits).unwrap().unwrap()
    print(second.target, second.minor, next.buffered())?
    print(std.http.typed.read_request(&mut next, &limits).unwrap().is_none())?
    Ok(())
"#,
        "GET /one 1 2 True\n2 True\nAB\n/two 0 0\nTrue\n",
        true,
    );
}

#[test]
fn typed_head_failures_keep_exact_consumed_progress_and_transport_payloads() {
    execute_modules(
        &format!(
            r#"{CHUNK_SOURCE}
import std.http
import std.http.typed
def main() -> Result[(), IoError]:
    text = "HTTP/1.1 200 OK\r\nX: yes\r\n\r\n"
    size = text.byte_len()
    limits = std.http.limits()
    for position in range(size):
        mut input = wire(text).unwrap()
        input.fail_at = position
        input.mode = 1
        match std.http.typed.read_response(&mut input, &limits):
            case Err(partial):
                match partial.cause:
                    case FramingError.Transport(Transport.Tls(47, "body")):
                        if partial.completed != position:
                            print("wrong progress", position, partial.completed)?
                    case _:
                        print("lost transport")?
            case Ok(_):
                print("missed failure")?
    mut complete = wire(text).unwrap()
    answer = std.http.typed.read_response(&mut complete, &limits).unwrap().unwrap()
    print(answer.status, answer.headers[0].value, complete.position == size)?
    mut partial = wire("HTTP/1.1 200 OK\r\nX: yes\r\n").unwrap()
    match std.http.typed.read_response(&mut partial, &limits):
        case Err(failure):
            print(failure.completed, reason(failure.cause))?
        case Ok(_):
            print("wrong")?
    mut bad = wire("HTTP/1.1 200 OK\n").unwrap()
    match std.http.typed.read_response(&mut bad, &limits):
        case Err(failure):
            print(failure.completed, reason(failure.cause))?
        case Ok(_):
            print("wrong")?
    Ok(())
"#
        ),
        "200 yes True\n25 truncated\n16 lf\n",
        true,
    );
}

#[test]
fn typed_head_writes_keep_partial_delivery_and_validate_before_writing() {
    execute_modules(
        r#"
import std.http
import std.http.typed
def main() -> Result[(), IoError]:
    head = std.http.request("GET", "/").unwrap()
    mut sink = Sink(1, 0, 0)
    match std.http.typed.write_request(&mut sink, &head):
        case Err(partial):
            match partial.cause:
                case FramingError.Transport(Storage.Full(code)):
                    print(partial.completed, code, sink.delivered)?
                case _:
                    print("lost cause")?
        case Ok(_):
            print("wrong")?
    mut invalid = std.http.request("GET bad", "/").unwrap()
    invalid.add("Host", "example").unwrap()
    mut untouched = Sink(0, 0, 0)
    match std.http.typed.write_request(&mut untouched, &invalid):
        case Err(partial):
            print(partial.completed, untouched.calls)?
        case Ok(_):
            print("wrong")?
    Ok(())
"#,
        "1 28 1\n0 0\n",
        true,
    );
}

#[test]
fn typed_head_limits_and_legacy_grammar_agree() {
    execute_modules(
        &format!(
            r#"{CHUNK_SOURCE}
import std.http
import std.http.typed
import std.io
import std.io.typed
def main() -> Result[(), IoError]:
    cases = ["HTTP/2.0 200 OK\r\n\r\n", "HTTP/1.1 099 No\r\n\r\n", "HTTP/1.1 20x No\r\n\r\n", "HTTP/1.1 200 OK\r\n bad: fold\r\n\r\n", "HTTP/1.1 200 OK\r\nBad : field\r\n\r\n", "HTTP/1.1 200 OK\r\nX: yes\n\r\n"].unwrap()
    limits = std.http.limits()
    for text in &cases:
        mut old = std.io.buffered_reader(std.io.cursor(std.bytes.from_text(text).unwrap()), 64).unwrap()
        mut typed = wire(*text).unwrap()
        legacy = std.http.read_response(&mut old, &limits)
        current = std.http.typed.read_response(&mut typed, &limits)
        match legacy:
            case Err(expected):
                match current:
                    case Err(failure):
                        match failure.cause:
                            case FramingError.Protocol(actual):
                                print(str.repr(expected).unwrap() == str.repr(actual).unwrap())?
                            case _:
                                print(False)?
                    case Ok(_):
                        print(False)?
            case Ok(_):
                print(False)?
    choices = [Limits(16, 256, 5, 0), Limits(17, 17, 5, 0), Limits(64, 256, 0, 0), Limits(1, 256, 5, 0)].unwrap()
    for limit in &choices:
        mut source = wire("HTTP/1.1 200 OK\r\nX: yes\r\n\r\n").unwrap()
        match std.http.typed.read_response(&mut source, limit):
            case Err(failure):
                print(failure.completed, source.position)?
            case Ok(_):
                print("wrong")?
    Ok(())
"#
        ),
        "True\nTrue\nTrue\nTrue\nTrue\nTrue\n16 16\n17 17\n25 25\n0 0\n",
        true,
    );
}

#[test]
fn typed_buffering_retains_payloads_and_checks_progress_without_empty_reads() {
    execute_modules(
        r#"
import std.io.typed
def main() -> Result[(), IoError]:
    mut source = std.io.typed.buffered_reader(Source(1, 0), 4).unwrap()
    mut bytes = scratch().unwrap()
    print(source.read(&mut bytes, 1, 1).unwrap(), source.inner.calls)?
    print(source.read(&mut bytes, -1, 1).is_err(), source.inner.calls)?
    match source.read(&mut bytes, 0, 1):
        case Err(StepError.Interrupted):
            print("retry")?
        case _:
            print("wrong")?
    print(source.read(&mut bytes, 0, 1).unwrap(), source.buffered(), source.inner.calls)?
    print(source.read(&mut bytes, 1, 2).unwrap(), source.buffered(), source.inner.calls)?
    match source.read(&mut bytes, 0, 1):
        case Err(StepError.Source(ReadFailure.Source(Transport.Tls(code, phase)))):
            print(code, phase)?
        case _:
            print("lost cause")?
    for mode in range(3, 5):
        mut invalid = std.io.typed.buffered_reader(Source(mode, 2), 4).unwrap()
        match invalid.read(&mut bytes, 0, 1):
            case Err(StepError.Source(ReadFailure.InvalidProgress)):
                print("invalid")?
            case _:
                print("wrong")?
    Ok(())
"#,
        "0 0\nTrue 0\nretry\n1 1 2\n1 0 2\n47 handshake\ninvalid\ninvalid\n",
        true,
    );
}

#[test]
fn parsed_head_bodies_validate_framing_limits_and_connection_policy() {
    execute_modules(
        &format!(
            r#"{CHUNK_SOURCE}
import std.http
import std.http.typed
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | HttpError | FramingError[Transport] | StepError[FramingError[Transport]] | CopyFailure[FramingError[Transport], Storage]]:
    limits = Limits(32, 128, 8, 2)
    mut head = std.http.response(200, "OK")?
    head.add("Content-Length", "3")?
    match std.http.typed.response_body(wire("@")?, &head, "GET", True, &limits):
        case Err(FramingError.Protocol(HttpError.BodyTooLarge)):
            print("large")?
        case _:
            print("wrong")?
    mut no_body = std.http.typed.response_body(wire("@")?, &head, "HEAD", True, &limits)?
    print(no_body.is_done(), no_body.total())?
    drop(no_body.finish()?)
    head.add("Transfer-Encoding", "chunked")?
    match std.http.typed.response_body(wire("@")?, &head, "HEAD", True, &limits):
        case Err(FramingError.Protocol(HttpError.ConflictingFraming)):
            print("conflict")?
        case _:
            print("wrong")?
    mut request = std.http.request("POST", "/")?
    request.add("Content-Length", "0")?
    mut empty = std.http.typed.request_body(wire("@")?, &request, &limits)?
    print(empty.is_done())?
    drop(empty.finish()?)
    request.add("Content-Length", "0")?
    match std.http.typed.request_body(wire("@")?, &request, &limits):
        case Err(FramingError.Protocol(HttpError.DuplicateFraming)):
            print("duplicate")?
        case _:
            print("wrong")?
    head = std.http.response(204, "No Content")?
    for index in range(7):
        mut policy = std.http.response(204, "No Content")?
        mut method = "GET"
        mut request_keep = True
        if index == 0:
            policy.add("Connection", "close")?
        elif index == 1:
            policy.minor = 0
        elif index == 2:
            policy.status = 101
        elif index == 3:
            method = "CONNECT"
        elif index == 4:
            request_keep = False
        elif index == 5:
            policy.minor = 0
            policy.add("Connection", "keep-alive")?
        mut body = std.http.typed.response_body(wire("@")?, &policy, &method, request_keep, &limits)?
        match body.finish():
            case Err(FramingError.NotReusable):
                print("close")?
            case Ok(_):
                print("keep")?
            case _:
                print("wrong")?
    match std.http.typed.response_body(wire("@")?, &head, "GET", True, &Limits(1, 128, 8, 2)):
        case Err(FramingError.Protocol(HttpError.InvalidLimit)):
            print("limit")?
        case _:
            print("wrong")?
    mut buffer = scratch()?
    mut sink = Sink(0, 0, 0)
    mut close_head = std.http.response(200, "OK")?
    mut exact = std.http.typed.response_body(wire("AB")?, &close_head, "GET", True, &limits)?
    print(copy_all(&mut exact, &mut sink, &mut buffer)?, exact.total(), exact.is_done())?
    match exact.finish():
        case Err(FramingError.NotReusable):
            print("closed")?
        case _:
            print("wrong")?
    mut large = std.http.typed.response_body(wire("ABC")?, &close_head, "GET", True, &limits)?
    match copy_all(&mut large, &mut sink, &mut buffer):
        case Err(CopyFailure.Read(count, ReadFailure.Source(FramingError.Protocol(HttpError.BodyTooLarge)))):
            print("large", count, large.total(), large.is_done())?
        case _:
            print("wrong")?
    match large.finish():
        case Err(FramingError.Broken):
            print("broken")?
        case _:
            print("wrong")?
    Ok(())
"#
        ),
        "large\nTrue 0\nconflict\nTrue\nduplicate\nclose\nclose\nclose\nclose\nclose\nkeep\nkeep\nlimit\n2 2 True\nclosed\nlarge 2 2 False\nbroken\n",
        true,
    );
}

#[test]
fn parsed_head_bodies_preserve_typed_causes_and_partial_progress() {
    execute_modules(
        &format!(
            r#"{CHUNK_SOURCE}
import std.http
import std.http.typed
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | HttpError | FramingError[Transport] | StepError[FramingError[Transport]]]:
    mut head = std.http.response(200, "OK")?
    head.add("Content-Length", "4")?
    limits = Limits(32, 128, 8, 4)
    mut buffer = scratch()?
    for mode in range(1, 6):
        mut body = std.http.typed.response_body(Source(mode, 0), &head, "GET", True, &limits)?
        match body.read(&mut buffer, -1, 1):
            case Err(StepError.Source(FramingError.OutOfRange)):
                pass
            case _:
                print("wrong")?
        print(body.read(&mut buffer, 0, 0)?, body.total())?
        match read_exact(&mut body, &mut buffer, 0, 4):
            case Err(partial):
                print(partial.completed, body.total())?
                match partial.cause:
                    case ReadFailure.Source(FramingError.Transport(Transport.Tls(code, message))):
                        print(code, message)?
                    case ReadFailure.Source(cause):
                        print(reason(cause))?
                    case _:
                        print("wrong")?
            case _:
                print("wrong")?
        match body.finish():
            case Err(FramingError.Broken):
                print("broken")?
            case _:
                print("wrong")?
    Ok(())
"#
        ),
        "0 0\n2 2\n47 handshake\nbroken\n0 0\n2 2\ncancelled\nbroken\n0 0\n2 2\nprogress\nbroken\n0 0\n2 2\nprogress\nbroken\n0 0\n2 2\nallocation\nbroken\n",
        true,
    );
}

#[test]
fn parsed_head_body_connection_ownership_is_released_exactly_once() {
    execute_modules(
        &format!(
            r#"{CHUNK_SOURCE}
import std.http
import std.http.typed
pub class Tracked:
    source: Wire
    tag: str
    def __del__(self) -> ():
        print(self.tag).unwrap()
    pub def read(self: &mut Tracked, buffer: &mut Bytes, start: i64, stop: i64) -> Result[i64, StepError[Transport]]:
        self.source.read(buffer, start, stop)
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | HttpError | FramingError[Transport] | CopyFailure[FramingError[Transport], Storage]]:
    limits = Limits(32, 128, 8, 2)
    mut head = std.http.response(200, "OK")?
    head.add("Content-Length", "9")?
    drop(std.http.typed.response_body(Tracked(wire("@")?, "construction"), &head, "GET", True, &limits))
    head.headers[0].value = "2"
    unfinished = std.http.typed.response_body(Tracked(wire("AB@")?, "unfinished"), &head, "GET", True, &limits)?
    drop(unfinished)
    mut buffer = scratch()?
    mut sink = Sink(0, 0, 0)
    mut broken = std.http.typed.response_body(Tracked(wire("A")?, "truncated"), &head, "GET", True, &limits)?
    match copy_all(&mut broken, &mut sink, &mut buffer):
        case Err(CopyFailure.Read(count, ReadFailure.Source(FramingError.Truncated))):
            print(count)?
        case _:
            print("wrong")?
    drop(broken.finish())
    drop(broken)
    mut body = std.http.typed.response_body(Tracked(wire("AB@")?, "recovered"), &head, "GET", True, &limits)?
    print(copy_all(&mut body, &mut sink, &mut buffer)?)?
    recovered = body.finish()?
    drop(body)
    print("owner retained")?
    drop(recovered)
    mut closing = std.http.typed.response_body(Tracked(wire("AB@")?, "closing"), &head, "GET", False, &limits)?
    print(copy_all(&mut closing, &mut sink, &mut buffer)?)?
    drop(closing.finish())
    print("still owned")?
    drop(closing)
    Ok(())
"#
        ),
        "construction\nunfinished\n1\ntruncated\n2\nowner retained\nrecovered\n2\nstill owned\nclosing\n",
        true,
    );
}

#[test]
fn chunked_body_preserves_interruptions_trailers_and_message_boundary() {
    execute_modules(
        &format!(
            r#"{CHUNK_SOURCE}
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | FramingError[Transport] | CopyFailure[FramingError[Transport], Storage] | StepError[FramingError[Transport]]]:
    mut body = chunked_body(wire("2; x=y\r\nAB\r\n1\r\nC\r\n0\r\nX-Tag: yes \t\r\n\r\n@")?, &Limits(32, 32, 1, 3))?
    mut buffer = scratch()?
    match body.read(&mut buffer, -1, 0):
        case Err(StepError.Source(FramingError.OutOfRange)):
            print("range")?
        case _:
            print("wrong")?
    print(body.read(&mut buffer, 2, 2)?)?
    print(body.is_done())?
    mut sink = Sink(0, 0, 0)
    print(copy_all(&mut body, &mut sink, &mut buffer)?)?
    print(body.total(), body.is_done())?
    print(len(body.trailers), body.trailers[0].name, body.trailers[0].value)?
    print(body.read(&mut buffer, 0, 4)?)?
    print(body.read(&mut buffer, 0, 4)?)?
    Ok(())
"#
        ),
        "range\n0\nFalse\n3\n3 True\n1 X-Tag yes\n0\n0\n",
        true,
    );
}

#[test]
fn chunked_body_rejects_malformed_truncated_and_over_limit_framing() {
    let cases = [
        ("", "32, 32, 2, 8", 0, "truncated"),
        ("1\r", "32, 32, 2, 8", 0, "truncated"),
        ("1\nA", "32, 32, 2, 8", 0, "lf"),
        ("z\r\n", "32, 32, 2, 8", 0, "chunk"),
        ("\r\n", "32, 32, 2, 8", 0, "chunk"),
        ("8000000000000000\r\n", "32, 32, 2, 8", 0, "chunk"),
        ("1 \r\n", "32, 32, 2, 8", 0, "chunk"),
        ("1;x\u{7f}\r\n", "32, 32, 2, 8", 0, "chunk"),
        ("2\r\nA", "32, 32, 2, 8", 1, "truncated"),
        ("1\r\nA", "32, 32, 2, 8", 1, "truncated"),
        ("1\r\nAx\r\n", "32, 32, 2, 8", 1, "chunk"),
        ("1\r\nA\r\n0\r\n", "32, 32, 2, 8", 1, "truncated"),
        ("0\r\n Fold: x\r\n\r\n", "32, 32, 2, 8", 0, "header"),
        ("0\r\nX : x\r\n\r\n", "32, 32, 2, 8", 0, "header"),
        ("0\r\nX: \u{1}\r\n\r\n", "32, 32, 2, 8", 0, "header"),
        ("0\r\nX: y\r\n\r\n", "32, 32, 0, 8", 0, "headers"),
        ("0\r\nX: y\r\n\r\n", "32, 7, 2, 8", 0, "head"),
        ("01\r\n", "3, 32, 2, 8", 0, "line"),
        ("2\r\nAB\r\n1\r\n", "32, 32, 2, 2", 2, "body"),
    ];
    let mut body = format!(
        "{CHUNK_SOURCE}\ndef main() -> Result[(), AllocError | IoError | std.bytes.BytesError | FramingError[Transport]]:\n    mut buffer = scratch()?\n"
    );
    let mut expected = String::new();
    for (index, (wire, limits, count, reason)) in cases.iter().enumerate() {
        let literal = format!("{wire:?}")
            .replace("\\u{7f}", "\u{7f}")
            .replace("\\u{1}", "\u{1}");
        body.push_str(&format!(
            r#"    mut body{index} = chunked_body(wire({literal})?, &Limits({limits}))?
    mut sink{index} = Sink(0, 0, 0)
    match copy_all(&mut body{index}, &mut sink{index}, &mut buffer):
        case Err(CopyFailure.Read(count, ReadFailure.Source(cause))):
            print(count, reason(cause))?
        case _:
            print("wrong")?
    match body{index}.read(&mut buffer, 0, 0):
        case Err(StepError.Source(FramingError.Broken)):
            print("broken")?
        case _:
            print("wrong")?
"#
        ));
        expected.push_str(&format!("{count} {reason}\nbroken\n"));
    }
    body.push_str("    Ok(())\n");
    execute_modules(&body, &expected, true);
}

#[test]
fn chunked_body_retains_transport_cause_and_payload_progress_at_every_phase() {
    execute_modules(
        &format!(
            r#"{CHUNK_SOURCE}
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | FramingError[Transport]]:
    mut buffer = scratch()?
    for offset in range(16):
        mut source = wire("1\r\nA\r\n0\r\nX:y\r\n\r\n")?
        source.fail_at = offset
        source.mode = 1
        mut body = chunked_body(source, &Limits(32, 32, 2, 8))?
        mut sink = Sink(0, 0, 0)
        match copy_all(&mut body, &mut sink, &mut buffer):
            case Err(CopyFailure.Read(count, ReadFailure.Source(FramingError.Transport(Transport.Tls(code, message))))):
                print(count, code, message)?
            case _:
                print("wrong")?
    for mode in range(2, 6):
        mut source = wire("2\r\nAB\r\n0\r\n\r\n")?
        source.fail_at = 4
        source.mode = mode
        mut body = chunked_body(source, &Limits(32, 32, 2, 8))?
        mut sink = Sink(0, 0, 0)
        match copy_all(&mut body, &mut sink, &mut buffer):
            case Err(CopyFailure.Read(count, ReadFailure.Source(cause))):
                print(count, reason(cause))?
            case _:
                print("wrong")?
    Ok(())
"#
        ),
        &format!(
            "{}{}1 cancelled\n1 progress\n1 progress\n1 allocation\n",
            "0 47 body\n".repeat(4),
            "1 47 body\n".repeat(12)
        ),
        true,
    );
}

#[test]
fn chunked_body_checks_constructor_limits_and_eof_at_every_boundary() {
    execute_modules(
        &format!(
            r#"{CHUNK_SOURCE}
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | FramingError[Transport] | CopyFailure[FramingError[Transport], Storage]]:
    mut buffer = scratch()?
    for mode in range(4):
        limits = Limits(1 if mode == 0 else 32, 1 if mode == 1 else 32, -1 if mode == 2 else 2, -1 if mode == 3 else 8)
        match chunked_body(wire("")?, &limits):
            case Err(FramingError.Protocol(HttpError.InvalidLimit)):
                print("limit")?
            case _:
                print("wrong")?
    for offset in range(16):
        text = "1\r\nA\r\n0\r\nX:y\r\n\r\n"
        mut body = chunked_body(wire(text.slice(0, offset)?)?, &Limits(32, 32, 2, 8))?
        mut sink = Sink(0, 0, 0)
        match copy_all(&mut body, &mut sink, &mut buffer):
            case Err(CopyFailure.Read(count, ReadFailure.Source(FramingError.Truncated))):
                print(count)?
            case _:
                print("wrong")?
    mut empty = chunked_body(wire("0\r\n\r\n@")?, &Limits(3, 2, 0, 0))?
    mut sink = Sink(0, 0, 0)
    print(copy_all(&mut empty, &mut sink, &mut buffer)?, empty.is_done())?
    Ok(())
"#
        ),
        &format!(
            "{}{}{}0 True\n",
            "limit\n".repeat(4),
            "0\n".repeat(4),
            "1\n".repeat(12)
        ),
        true,
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn chunked_body_allocation_failure_is_typed_and_terminal() {
    execute_modules(
        &format!(
            r#"{CHUNK_SOURCE}
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | FramingError[Transport]]:
    mut buffer = scratch()?
    mut body = chunked_body(wire("1\r\nA\r\n0\r\nX:y\r\n\r\n")?, &Limits(32, 32, 2, 8))?
    mut sink = Sink(0, 0, 0)
    print("__test_fail_allocations_after_0__")?
    result = copy_all(&mut body, &mut sink, &mut buffer)
    print("__test_restore_allocations__")?
    match result:
        case Err(CopyFailure.Read(count, ReadFailure.Source(FramingError.Protocol(HttpError.Allocation(AllocError.OutOfMemory))))):
            print(count)?
        case _:
            print("wrong")?
    match body.read(&mut buffer, 0, 4):
        case Err(StepError.Source(FramingError.Broken)):
            print("broken")?
        case _:
            print("wrong")?
    Ok(())
"#
        ),
        "1\nbroken\n",
        true,
    );
}

#[test]
fn legacy_bridges_retry_only_interruption_and_preserve_io_errors() {
    execute_modules(
        r#"
import std.io
import std.io.typed
pub class Legacy:
    calls: i64
    pub def read(self: &mut Legacy, buffer: &mut Bytes, start: i64, stop: i64) -> Result[i64, std.io.StreamError]:
        self.calls += 1
        if self.calls == 1:
            return Err(std.io.StreamError.Interrupted)
        if self.calls == 2:
            return Ok(1)
        Err(std.io.StreamError.Io(IoError.Other))
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:
    mut reader = std.io.typed.reader(Legacy(0))
    mut buffer = scratch()?
    match read_exact(&mut reader, &mut buffer, 0, 4):
        case Err(partial):
            print(partial.completed)?
            match partial.cause:
                case ReadFailure.Source(std.io.StreamError.Io(IoError.Other)):
                    print("io")?
                case _:
                    print("wrong")?
        case Ok(_):
            print("wrong")?
    original = std.io.typed.unwrap_reader(reader)
    print(original.calls)?
    Ok(())
"#,
        "1\nio\n3\n",
        true,
    );
}

#[test]
fn protocol_bounds_infer_errors_and_nested_framing_without_explicit_types() {
    execute(
        r#"
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | Partial[ReadFailure[Transport]] | FramingError[Transport] | CopyFailure[FramingError[Transport], Storage]]:
    mut buffer = scratch()?
    mut source = Source(0, 0)
    read_exact(&mut source, &mut buffer, 0, 2)?
    mut body = fixed_body(Source(0, 0), 2)?
    mut sink = Sink(0, 0, 0)
    print(copy_all(&mut body, &mut sink, &mut buffer)?)?
    Ok(())
"#,
        "2\n",
    );
}

#[test]
fn standalone_example_keeps_the_original_transport_code() {
    let workspace = tempfile::tempdir().unwrap();
    let binary = workspace.path().join("program");
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/error_composition/main.plenty");
    plenty::compile_file_to_executable(&file, &binary, None).unwrap();
    let output = Command::new(binary).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"1\n104\n");
}

#[test]
fn empty_and_invalid_ranges_do_not_touch_transports() {
    execute(
        r#"
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | Partial[WriteFailure[Storage]]]:
    mut source = Source(0, 0)
    mut sink = Sink(0, 0, 0)
    mut buffer = scratch()?
    write_all[Storage, Sink](&mut sink, &buffer, 2, 2)?
    match write_all[Storage, Sink](&mut sink, &buffer, 3, 5):
        case Err(partial):
            print(partial.completed == 0)?
            match partial.cause:
                case WriteFailure.OutOfRange:
                    print("range")?
                case _:
                    print("wrong")?
        case _:
            print("wrong")?
    buffer.clear()
    match copy_all[Transport, Storage, Source, Sink](&mut source, &mut sink, &mut buffer):
        case Err(CopyFailure.Read(0, ReadFailure.OutOfRange)):
            print("empty")?
        case _:
            print("wrong")?
    print(source.calls)?
    print(sink.calls)?
    match fixed_body[Transport, Source](Source(0, 0), -1):
        case Err(FramingError.OutOfRange):
            print("length")?
        case _:
            print("wrong")?
    Ok(())
"#,
        "True\nrange\nempty\n0\n0\nlength\n",
    );
}

#[test]
fn exact_read_retains_partial_progress_and_full_transport_cause() {
    execute(
        r#"
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:
    mut source = Source(1, 0)
    mut buffer = scratch()?
    match read_exact[Transport, Source](&mut source, &mut buffer, 0, 4):
        case Ok(_):
            print("unexpected")?
        case Err(partial):
            print(partial.completed)?
            match partial.cause:
                case ReadFailure.Source(Transport.Tls(code, phase)):
                    print(code)?
                    print(phase)?
                case _:
                    print("wrong cause")?
    print(buffer[0])?
    print(source.calls)?
    Ok(())
"#,
        "2\n47\nhandshake\n65\n3\n",
    );
}

#[test]
fn read_helpers_distinguish_eof_cancellation_invalid_progress_and_allocation() {
    execute(
        r#"
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:
    mut buffer = scratch()?
    for mode in range(6):
        mut source = Source(mode, 0)
        match read_exact[Transport, Source](&mut source, &mut buffer, 0, 4):
            case Ok(_):
                print("unexpected")?
            case Err(partial):
                print(partial.completed)?
                match partial.cause:
                    case ReadFailure.UnexpectedEof:
                        print("eof")?
                    case ReadFailure.InvalidProgress:
                        print("invalid")?
                    case ReadFailure.Source(Transport.Cancelled):
                        print("cancelled")?
                    case ReadFailure.Source(Transport.Allocation(AllocError.OutOfMemory)):
                        print("allocation")?
                    case _:
                        print("tls")?
    mut source = Source(0, 0)
    drop(read_exact[Transport, Source](&mut source, &mut buffer, 2, 2))
    drop(read_exact[Transport, Source](&mut source, &mut buffer, -1, 2))
    print(source.calls)?
    Ok(())
"#,
        "2\neof\n2\ntls\n2\ncancelled\n2\ninvalid\n2\ninvalid\n2\nallocation\n0\n",
    );
}

#[test]
fn copy_distinguishes_write_failure_and_partial_delivery() {
    execute(
        r#"
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:
    mut buffer = scratch()?
    for mode in range(5):
        mut source = Source(0, 0)
        mut sink = Sink(mode, 0, 0)
        match copy_all[Transport, Storage, Source, Sink](&mut source, &mut sink, &mut buffer):
            case Ok(count):
                print(count)?
            case Err(CopyFailure.Write(count, reason)):
                print(count)?
                match reason:
                    case WriteFailure.Source(Storage.Full(code)):
                        print(code)?
                    case WriteFailure.WriteZero:
                        print("zero")?
                    case WriteFailure.InvalidProgress:
                        print("invalid")?
                    case _:
                        print("wrong")?
            case _:
                print("wrong side")?
        print(sink.delivered)?
    Ok(())
"#,
        "2\n2\n1\n28\n1\n0\nzero\n0\n0\ninvalid\n0\n0\ninvalid\n0\n",
    );
}

#[test]
fn http_body_copy_has_one_error_path_and_preserves_transport_cause() {
    execute(
        r#"
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | FramingError[Transport] | CopyFailure[FramingError[Transport], Storage]]:
    mut buffer = scratch()?
    for mode in range(6):
        mut body = fixed_body[Transport, Source](Source(mode, 0), 4)?
        mut sink = Sink(0, 0, 0)
        match copy_all[FramingError[Transport], Storage, FixedBody[Transport, Source], Sink](&mut body, &mut sink, &mut buffer):
            case Err(CopyFailure.Read(count, ReadFailure.Source(cause))):
                print(count)?
                match cause:
                    case FramingError.Transport(Transport.Tls(code, phase)):
                        print(code)?
                        print(phase)?
                    case FramingError.Transport(Transport.Cancelled):
                        print("cancelled")?
                    case FramingError.Transport(Transport.Allocation(AllocError.OutOfMemory)):
                        print("allocation")?
                    case FramingError.Truncated:
                        print("truncated")?
                    case FramingError.InvalidProgress:
                        print("invalid")?
                    case _:
                        print("wrong")?
            case _:
                print("wrong path")?
        match body.read(&mut buffer, 0, 4):
            case Err(StepError.Source(FramingError.Broken)):
                print("broken")?
            case _:
                print("reused")?
    mut finished = fixed_body[Transport, Source](Source(1, 0), 2)?
    mut sink = Sink(0, 0, 0)
    print(copy_all[FramingError[Transport], Storage, FixedBody[Transport, Source], Sink](&mut finished, &mut sink, &mut buffer)?)?
    Ok(())
"#,
        "2\ntruncated\nbroken\n2\n47\nhandshake\nbroken\n2\ncancelled\nbroken\n2\ninvalid\nbroken\n2\ninvalid\nbroken\n2\nallocation\nbroken\n2\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn typed_stream_path_runs_without_allocating() {
    execute(
        r#"
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | FramingError[Transport] | CopyFailure[FramingError[Transport], Storage]]:
    mut buffer = scratch()?
    mut body = fixed_body[Transport, Source](Source(0, 0), 2)?
    mut sink = Sink(0, 0, 0)
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    result = copy_all[FramingError[Transport], Storage, FixedBody[Transport, Source], Sink](&mut body, &mut sink, &mut buffer)
    print("__test_end_no_allocations__")?
    print("__test_restore_allocations__")?
    print(result?)?
    Ok(())
"#,
        "2\n",
    );
}
