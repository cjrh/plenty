//! `std.http`: HTTP/1.1 message framing over in-memory streams and loopback
//! sockets. No test needs a network or a fixed port.
mod support;
use support::hex;

const HEADER: &str = "import std.bytes\nimport std.encoding\nimport std.io\nimport std.http\nfrom std.bytes import Bytes\nfrom std.io import BufReader, Cursor, Reader, StreamError\nfrom std.http import Framing, Header, HttpError, Limits\n";

/// `requests` and `responses` print one line per message, `eof` at a clean
/// end, or `err <error>` and stop. Each body is read through a 7-byte buffer
/// so that lines and chunks straddle refills.
const HELPERS: &str = r#"
def shown(result: Result[(), HttpError]) -> Result[(), AllocError | IoError]:
    match result:
        case Ok(_):
            pass
        case Err(error):
            print("err", str.repr(&error)?)?
    Ok(())

def body_text[R: Reader](body: &mut std.http.BodyReader[R]) -> Result[str, HttpError]:
    mut out = std.bytes.empty().unwrap()
    std.http.read_to_end(body, &mut out)?
    mut trailers = ""
    for index in range(len(body.trailers)):
        header = &body.trailers[index]
        trailers = trailers.concat(" ").unwrap().concat(&header.name).unwrap().concat("=").unwrap().concat(&header.value).unwrap()
    match out.to_text():
        case Ok(text):
            Ok(text.concat(trailers).unwrap())
        case Err(_):
            Ok(str.from(out.len()).unwrap().concat(" bytes").unwrap().concat(trailers).unwrap())

# Prints a request's checked target as `authority|path|query`.
def target_of(data: Bytes, limits: &Limits) -> Result[(), HttpError]:
    mut conn = std.io.buffered_reader(std.io.cursor(data), 7).unwrap()
    match std.http.read_request(&mut conn, limits)?:
        case Some(head):
            target = std.http.request_target(&head)?
            print(target.authority.concat("|").unwrap().concat(&target.path).unwrap().concat("|").unwrap().concat(&target.query).unwrap()).unwrap()
        case Nothing:
            print("eof").unwrap()
    Ok(())

def requests_from(data: Bytes, limits: &Limits) -> Result[(), HttpError]:
    mut conn = std.io.buffered_reader(std.io.cursor(data), 7).unwrap()
    while True:
        match std.http.read_request(&mut conn, limits)?:
            case Nothing:
                print("eof").unwrap()
                return Ok(())
            case Some(head):
                framing = std.http.request_framing(&head)?
                mut body = std.http.body_reader(conn, &framing, limits)?
                text = body_text(&mut body)?
                print("req", head.method, head.target, head.minor, len(head.headers), str.repr(&framing).unwrap(), std.http.request_keeps_alive(&head), str.repr(&text).unwrap()).unwrap()
                conn = std.http.finish(&mut body)?
    Ok(())

def responses_from(data: Bytes, method: &str, limits: &Limits) -> Result[(), HttpError]:
    mut conn = std.io.buffered_reader(std.io.cursor(data), 7).unwrap()
    while True:
        match std.http.read_response(&mut conn, limits)?:
            case Nothing:
                print("eof").unwrap()
                return Ok(())
            case Some(head):
                framing = std.http.response_framing(&head, method)?
                mut body = std.http.body_reader(conn, &framing, limits)?
                text = body_text(&mut body)?
                print("resp", head.minor, head.status, str.repr(&head.reason).unwrap(), str.repr(&framing).unwrap(), std.http.response_keeps_alive(&head, &framing), str.repr(&text).unwrap()).unwrap()
                conn = std.http.finish(&mut body)?
    Ok(())

def responses_with_get(data: Bytes, limits: &Limits) -> Result[(), HttpError]:
    responses_from(data, "GET", limits)

# Reads every request without printing, for allocation-failure tests.
def drain(data: &Bytes) -> Result[i64, HttpError]:
    match data.copy():
        case Ok(copied):
            match std.io.buffered_reader(std.io.cursor(copied), 16):
                case Ok(conn):
                    drain_from(conn)
                case Err(error):
                    match error:
                        case StreamError.Allocation(cause):
                            Err(HttpError.Allocation(cause))
                        case _ as other:
                            Err(HttpError.Stream(other))
        case Err(error):
            match error:
                case std.bytes.BytesError.Allocation(cause):
                    Err(HttpError.Allocation(cause))
                case _:
                    Err(HttpError.Truncated)

def drain_from(start: BufReader[Cursor]) -> Result[i64, HttpError]:
    limits = std.http.limits()
    mut conn = start
    mut count = 0
    while True:
        match std.http.read_request(&mut conn, &limits)?:
            case Nothing:
                return Ok(count)
            case Some(head):
                framing = std.http.request_framing(&head)?
                mut body = std.http.body_reader(conn, &framing, &limits)?
                count += std.http.discard(&mut body)?
                conn = std.http.finish(&mut body)?
    Ok(count)

def requests(text: &str) -> Result[(), std.bytes.BytesError | AllocError | IoError]:
    shown(requests_from(std.bytes.from_text(text)?, &std.http.limits()))

def responses(text: &str, method: &str) -> Result[(), std.bytes.BytesError | AllocError | IoError]:
    shown(responses_from(std.bytes.from_text(text)?, method, &std.http.limits()))

def written(result: Result[(), HttpError], sink: &Cursor) -> Result[(), std.bytes.BytesError | AllocError | IoError]:
    match result:
        case Ok(_):
            print(str.repr(sink.data.to_text()?)?)?
        case Err(error):
            print("err", str.repr(&error)?)?
    Ok(())

def tag[T](result: &Result[T, HttpError]) -> str:
    match result:
        case Ok(_):
            "ok"
        case Err(error):
            match error:
                case HttpError.Allocation(_):
                    "alloc"
                case _:
                    "other"
"#;

fn program(body: &str) -> String {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    format!(
        "{HEADER}{HELPERS}\ndef main() -> Result[(), std.bytes.BytesError | std.encoding.EncodingError | std.io.StreamError | AllocError | IoError]:\n{body}    Ok(())\n"
    )
}

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
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("std.http.HttpError.", "")
        .replace("std.http.Framing.", "")
        .replace("std.io.StreamError.", "")
}

fn native(body: &str, expected: &str) {
    assert_eq!(
        output(&program(body)),
        expected.trim_matches('\n'),
        "{body}"
    );
}

/// Runs `helper` over every input in one program. Inputs travel as hex so
/// they may hold any bytes; `0xff` never occurs in them and ends each one.
fn each(helper: &str, inputs: &[&[u8]]) -> Vec<String> {
    let mut blob = String::new();
    for input in inputs {
        assert!(!input.contains(&0xff));
        blob.push_str(&hex(input));
        blob.push_str("ff");
    }
    let body = format!(
        r#"
blob = std.encoding.hex_from_text("{blob}")?
mut start = 0
while start < blob.len():
    stop = blob.find(255u8, start, blob.len())?.unwrap()
    print("--")?
    shown({helper}(blob.slice(start, stop)?, &std.http.limits()))?
    start = stop + 1
"#
    );
    let text = output(&program(&body));
    let cases: Vec<String> = text
        .split("--\n")
        .skip(1)
        .map(|case| case.trim_end().to_string())
        .collect();
    assert_eq!(cases.len(), inputs.len(), "{text}");
    cases
}

/// Every input must fail with `expected` before any message completes.
fn rejected(helper: &str, cases: &[(&[u8], &str)]) {
    let inputs: Vec<&[u8]> = cases.iter().map(|(input, _)| *input).collect();
    for ((input, expected), actual) in cases.iter().zip(each(helper, &inputs)) {
        assert_eq!(
            actual,
            format!("err {expected}"),
            "{:?}",
            String::from_utf8_lossy(input)
        );
    }
}

#[test]
fn pipelined_requests_keep_the_connection_positioned() {
    native(
        r#"
requests("GET /a HTTP/1.1\r\nHost: x\r\n\r\nPOST /b HTTP/1.1\r\nhost: x\r\ncontent-length: 5\r\n\r\nhelloPUT /c HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n3;name=value\r\nabc\r\nA\r\n0123456789\r\n0\r\nX-Sum: 13\r\nX-Other: \r\n\r\nGET / HTTP/1.0\r\n\r\n")?
"#,
        r#"
req GET /a 1 1 Empty True ""
req POST /b 1 2 Length(5) True "hello"
req PUT /c 1 1 Chunked True "abc0123456789 X-Sum=13 X-Other="
req GET / 0 0 Empty False ""
eof
"#,
    );
}

#[test]
fn connection_tokens_decide_reuse() {
    native(
        r#"
requests("GET / HTTP/1.1\r\nConnection: keep-alive, Close\r\n\r\nGET / HTTP/1.0\r\nConnection: Keep-Alive\r\n\r\nGET / HTTP/1.1\r\nConnection: upgrade\r\nConnection: close\r\n\r\n")?
responses("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\nHTTP/1.0 200 OK\r\nConnection: keep-alive\r\nContent-Length: 1\r\n\r\nxHTTP/1.1 200 OK\r\n\r\nall of the rest", "GET")?
"#,
        r#"
req GET / 1 1 Empty False ""
req GET / 0 1 Empty True ""
req GET / 1 2 Empty False ""
eof
resp 1 200 "OK" Length(0) True ""
resp 0 200 "OK" Length(1) True "x"
resp 1 200 "OK" UntilClose False "all of the rest"
eof
"#,
    );
}

#[test]
fn leading_blank_lines_before_a_request_are_skipped() {
    native(
        r#"
requests("\r\n\r\nGET / HTTP/1.1\r\n\r\n")?
requests("")?
"#,
        "req GET / 1 0 Empty True \"\"\neof\neof",
    );
}

#[test]
fn response_framing_follows_the_method_and_status() {
    native(
        r#"
responses("HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok", "POST")?
responses("HTTP/1.1 200 OK\r\nContent-Length: 2000\r\n\r\nHTTP/1.1 404 Not Found\r\nTransfer-Encoding: chunked\r\n\r\n", "HEAD")?
responses("HTTP/1.1 204 No Content\r\n\r\nHTTP/1.1 304 Not Modified\r\nContent-Length: 9\r\n\r\n", "GET")?
responses("HTTP/1.1 200 Connection established\r\n\r\n", "CONNECT")?
responses("HTTP/1.1 407 Proxy Authentication Required\r\nContent-Length: 1\r\n\r\nx", "CONNECT")?
responses("HTTP/1.1 999 \r\n\r\n", "GET")?
responses("HTTP/1.1 200\r\nContent-Length: 0\r\n\r\n", "GET")?
"#,
        r#"
resp 1 100 "Continue" Empty True ""
resp 1 200 "OK" Length(2) True "ok"
eof
resp 1 200 "OK" Empty True ""
resp 1 404 "Not Found" Empty True ""
eof
resp 1 204 "No Content" Empty True ""
resp 1 304 "Not Modified" Empty True ""
eof
resp 1 200 "Connection established" Empty True ""
eof
resp 1 407 "Proxy Authentication Required" Length(1) True "x"
eof
resp 1 999 "" UntilClose False ""
eof
resp 1 200 "" Length(0) True ""
eof
"#,
    );
}

#[test]
fn malformed_request_heads_are_rejected() {
    rejected(
        "requests_from",
        &[
            (b"GET / HTTP/1.1\nHost: x\r\n\r\n", "BareLineFeed"),
            (b"GET / HTTP/1.1\r\nHost: x\n\r\n", "BareLineFeed"),
            (
                b"GET / HTTP/1.1\r\nHost: x\r\n folded\r\n\r\n",
                "InvalidHeader",
            ),
            (b"GET / HTTP/1.1\r\nHost : x\r\n\r\n", "InvalidHeader"),
            (b"GET / HTTP/1.1\r\n: x\r\n\r\n", "InvalidHeader"),
            (b"GET / HTTP/1.1\r\nHost x\r\n\r\n", "InvalidHeader"),
            (b"GET / HTTP/1.1\r\nHo(st: x\r\n\r\n", "InvalidHeader"),
            (b"GET / HTTP/1.1\r\nHost: a\x00b\r\n\r\n", "InvalidHeader"),
            (b"GET / HTTP/1.1\r\nHost: a\rb\r\n\r\n", "InvalidHeader"),
            (b"GET / HTTP/1.1\r\nHost: \xc3\x28\r\n\r\n", "InvalidHeader"),
            (b"GET  / HTTP/1.1\r\n\r\n", "InvalidRequestLine"),
            (b"GET / HTTP/1.1 \r\n\r\n", "InvalidRequestLine"),
            (b"GET /\r\n\r\n", "InvalidRequestLine"),
            (b"G@T / HTTP/1.1\r\n\r\n", "InvalidRequestLine"),
            (b"GET /a\x7fb HTTP/1.1\r\n\r\n", "InvalidRequestLine"),
            (b"GET / http/1.1\r\n\r\n", "InvalidRequestLine"),
            (b"GET / HTTP/1.10\r\n\r\n", "InvalidRequestLine"),
            (b"GET / HTTP/2.0\r\n\r\n", "UnsupportedVersion"),
            (b"GET / HTTP/0.9\r\n\r\n", "UnsupportedVersion"),
            (b"GET / HTTP/1.1\r\nHost: x\r\n", "Truncated"),
            (b"GET / HTTP/1.1\r\nHost: x", "Truncated"),
            (b"GET / HTTP/1.1\r", "Truncated"),
        ],
    );
}

/// RFC 9112 section 3.2: exactly one valid Host for HTTP/1.1, at most one for
/// HTTP/1.0, and an absolute-form authority in place of Host.
#[test]
fn request_targets_and_host_are_checked() {
    let accepted: &[(&[u8], &str)] = &[
        (b"GET / HTTP/1.1\r\nHost: x\r\n\r\n", "x|/|"),
        (
            b"GET /a/b?c=1?d HTTP/1.1\r\nhost: example.com:8080\r\n\r\n",
            "example.com:8080|/a/b|c=1?d",
        ),
        (b"GET / HTTP/1.0\r\n\r\n", "|/|"),
        (b"GET / HTTP/1.1\r\nHost:\r\n\r\n", "|/|"),
        (b"GET / HTTP/1.1\r\nHost: [::1]:80\r\n\r\n", "[::1]:80|/|"),
        (b"GET / HTTP/1.1\r\nHost: [v1.a:b]\r\n\r\n", "[v1.a:b]|/|"),
        (b"GET / HTTP/1.1\r\nHost: [v1.x]:80\r\n\r\n", "[v1.x]:80|/|"),
        (b"GET / HTTP/1.1\r\nHost: [VaF.:]\r\n\r\n", "[VaF.:]|/|"),
        (
            b"GET / HTTP/1.1\r\nHost: a%2Fb.c_d~e!$&'()*+,;=\r\n\r\n",
            "a%2Fb.c_d~e!$&'()*+,;=|/|",
        ),
        (
            b"GET http://Example.com/p?q HTTP/1.1\r\nHost: other\r\n\r\n",
            "Example.com|/p|q",
        ),
        (
            b"GET HTTPS://x:8443 HTTP/1.1\r\nHost: x\r\n\r\n",
            "x:8443|/|",
        ),
        (b"GET http://x?q HTTP/1.0\r\n\r\n", "x|/|q"),
    ];
    let inputs: Vec<&[u8]> = accepted.iter().map(|(input, _)| *input).collect();
    for ((input, expected), actual) in accepted.iter().zip(each("target_of", &inputs)) {
        assert_eq!(&actual, expected, "{:?}", String::from_utf8_lossy(input));
    }
    rejected(
        "target_of",
        &[
            (b"GET / HTTP/1.1\r\n\r\n", "MissingHost"),
            (b"GET http://x/ HTTP/1.1\r\n\r\n", "MissingHost"),
            (
                b"GET / HTTP/1.1\r\nHost: x\r\nHost: x\r\n\r\n",
                "DuplicateHost",
            ),
            (
                b"GET / HTTP/1.0\r\nHost: x\r\nHOST: y\r\n\r\n",
                "DuplicateHost",
            ),
            (b"GET / HTTP/1.1\r\nHost: a b\r\n\r\n", "InvalidHost"),
            (b"GET / HTTP/1.1\r\nHost: u@x\r\n\r\n", "InvalidHost"),
            (b"GET / HTTP/1.1\r\nHost: x/y\r\n\r\n", "InvalidHost"),
            (b"GET / HTTP/1.1\r\nHost: x:8o\r\n\r\n", "InvalidHost"),
            (b"GET / HTTP/1.1\r\nHost: a%2\r\n\r\n", "InvalidHost"),
            (b"GET / HTTP/1.1\r\nHost: [::1\r\n\r\n", "InvalidHost"),
            (b"GET / HTTP/1.1\r\nHost: []\r\n\r\n", "InvalidHost"),
            (b"GET / HTTP/1.1\r\nHost: x[::1]\r\n\r\n", "InvalidHost"),
            (b"GET / HTTP/1.1\r\nHost: [::g]\r\n\r\n", "InvalidHost"),
            (b"GET / HTTP/1.1\r\nHost: [v]\r\n\r\n", "InvalidHost"),
            (b"GET / HTTP/1.1\r\nHost: [vx]\r\n\r\n", "InvalidHost"),
            (b"GET / HTTP/1.1\r\nHost: [v1]\r\n\r\n", "InvalidHost"),
            (b"GET / HTTP/1.1\r\nHost: [v1.]\r\n\r\n", "InvalidHost"),
            (b"GET / HTTP/1.1\r\nHost: [vx.a]\r\n\r\n", "InvalidHost"),
            (b"GET / HTTP/1.1\r\nHost: [v1x]\r\n\r\n", "InvalidHost"),
            (b"GET / HTTP/1.1\r\nHost: [v.a]\r\n\r\n", "InvalidHost"),
            (
                b"GET http://[vx]/ HTTP/1.1\r\nHost: x\r\n\r\n",
                "InvalidTarget",
            ),
            (b"GET / HTTP/1.1\r\nHost: h\xc3\xa9\r\n\r\n", "InvalidHost"),
            (
                b"GET http://x/ HTTP/1.1\r\nHost: a b\r\n\r\n",
                "InvalidHost",
            ),
            (b"GET * HTTP/1.1\r\nHost: x\r\n\r\n", "InvalidTarget"),
            (
                b"CONNECT x:443 HTTP/1.1\r\nHost: x:443\r\n\r\n",
                "InvalidTarget",
            ),
            (b"GET ftp://x/ HTTP/1.1\r\nHost: x\r\n\r\n", "InvalidTarget"),
            (
                b"GET http://u@x/ HTTP/1.1\r\nHost: x\r\n\r\n",
                "InvalidTarget",
            ),
            (
                b"GET http:///p HTTP/1.1\r\nHost: x\r\n\r\n",
                "InvalidTarget",
            ),
            (
                b"GET http://:80/ HTTP/1.1\r\nHost: x\r\n\r\n",
                "InvalidTarget",
            ),
            (b"GET http:/x HTTP/1.1\r\nHost: x\r\n\r\n", "InvalidTarget"),
        ],
    );
}

#[test]
fn ambiguous_request_framing_is_rejected() {
    rejected(
        "requests_from",
        &[
            (b"POST / HTTP/1.1\r\nContent-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\n", "ConflictingFraming"),
            (b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\nContent-Length: 1\r\n\r\n", "ConflictingFraming"),
            (b"POST / HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\nx", "DuplicateFraming"),
            (b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\nTransfer-Encoding: chunked\r\n\r\n", "DuplicateFraming"),
            (b"POST / HTTP/1.1\r\nContent-Length: 1,1\r\n\r\nx", "InvalidContentLength"),
            (b"POST / HTTP/1.1\r\nContent-Length: +5\r\n\r\nhello", "InvalidContentLength"),
            (b"POST / HTTP/1.1\r\nContent-Length: -1\r\n\r\n", "InvalidContentLength"),
            (b"POST / HTTP/1.1\r\nContent-Length: 0x5\r\n\r\n", "InvalidContentLength"),
            (b"POST / HTTP/1.1\r\nContent-Length: \r\n\r\n", "InvalidContentLength"),
            (b"POST / HTTP/1.1\r\nContent-Length: 99999999999999999999\r\n\r\n", "InvalidContentLength"),
            (b"POST / HTTP/1.1\r\nContent-Length: 9223372036854775808\r\n\r\n", "InvalidContentLength"),
            (b"POST / HTTP/1.1\r\nContent-Length: 1048577\r\n\r\n", "BodyTooLarge"),
            (b"POST / HTTP/1.1\r\nTransfer-Encoding: gzip, chunked\r\n\r\n", "UnsupportedTransferEncoding"),
            (b"POST / HTTP/1.1\r\nTransfer-Encoding: gzip\r\n\r\n", "UnsupportedTransferEncoding"),
            (b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked, \r\n\r\n", "UnsupportedTransferEncoding"),
            (b"POST / HTTP/1.0\r\nTransfer-Encoding: chunked\r\n\r\n", "UnsupportedTransferEncoding"),
        ],
    );
}

#[test]
fn malformed_bodies_are_rejected() {
    rejected(
        "requests_from",
        &[
            (b"POST / HTTP/1.1\r\nContent-Length: 5\r\n\r\nhell", "Truncated"),
            (b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhel", "Truncated"),
            (b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n", "Truncated"),
            (b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n", "Truncated"),
            (b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n", "InvalidChunk"),
            (b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n\r\n", "InvalidChunk"),
            (b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n-1\r\n", "InvalidChunk"),
            (b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n 5\r\nhello\r\n0\r\n\r\n", "InvalidChunk"),
            (b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5;a\x00\r\nhello\r\n0\r\n\r\n", "InvalidChunk"),
            (b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n8000000000000000\r\n", "InvalidChunk"),
            (b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhelloXX0\r\n\r\n", "InvalidChunk"),
            (b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\nhello\r\n0\r\n\r\n", "BareLineFeed"),
            (b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n100001\r\n", "BodyTooLarge"),
            (b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nBad Trailer\r\n\r\n", "InvalidHeader"),
        ],
    );
}

#[test]
fn malformed_responses_are_rejected() {
    rejected(
        "responses_with_get",
        &[
            (b"HTTP/1.1 20 OK\r\n\r\n", "InvalidStatusLine"),
            (b"HTTP/1.1 2000 OK\r\n\r\n", "InvalidStatusLine"),
            (b"HTTP/1.1 099 OK\r\n\r\n", "InvalidStatusLine"),
            (b"HTTP/1.1 200x\r\n\r\n", "InvalidStatusLine"),
            (b"HTTP/1.1  200 OK\r\n\r\n", "InvalidStatusLine"),
            (b"HTTP/1.1 200 O\x01K\r\n\r\n", "InvalidStatusLine"),
            (b"HTTP/2 200 OK\r\n\r\n", "InvalidStatusLine"),
            (b"HTTP/3.0 200 OK\r\n\r\n", "UnsupportedVersion"),
            (b"\r\nHTTP/1.1 200 OK\r\n\r\n", "InvalidStatusLine"),
            (
                b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Length: 2\r\n\r\nok",
                "DuplicateFraming",
            ),
            (
                b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nok",
                "Truncated",
            ),
        ],
    );
}

#[test]
fn limits_bound_lines_heads_counts_and_bodies() {
    native(
        r#"
small = Limits(32, 64, 2, 8)
long_line = std.bytes.from_text("GET /aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa HTTP/1.1\r\n\r\n")?
shown(requests_from(long_line, &small))?
big_head = std.bytes.from_text("GET / HTTP/1.1\r\nA: aaaaaaaaaaaaaaaaaaaa\r\nB: bbbbbbbbbbbbbbbbbbbb\r\n\r\n")?
shown(requests_from(big_head, &small))?
blank = std.bytes.from_text("\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\nGET / HTTP/1.1\r\n\r\n")?
shown(requests_from(blank, &small))?
many = std.bytes.from_text("GET / HTTP/1.1\r\nA: 1\r\nB: 2\r\nC: 3\r\n\r\n")?
shown(requests_from(many, &small))?
fits = std.bytes.from_text("GET / HTTP/1.1\r\nA: 1\r\nB: 2\r\n\r\n")?
shown(requests_from(fits, &small))?
sized = std.bytes.from_text("POST / HTTP/1.1\r\nContent-Length: 9\r\n\r\n123456789")?
shown(requests_from(sized, &small))?
chunked = std.bytes.from_text("POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n1234\r\n4\r\n5678\r\n1\r\n9\r\n0\r\n\r\n")?
shown(requests_from(chunked, &small))?
exact = std.bytes.from_text("POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n1234\r\n4\r\n5678\r\n0\r\n\r\n")?
shown(requests_from(exact, &small))?
trailers = std.bytes.from_text("POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nA: 1\r\nB: 2\r\nC: 3\r\n\r\n")?
shown(requests_from(trailers, &small))?
close = std.bytes.from_text("HTTP/1.1 200 OK\r\n\r\n123456789")?
shown(responses_from(close, "GET", &small))?
shown(requests_from(std.bytes.empty()?, &Limits(0, 64, 2, 8)))?
shown(requests_from(std.bytes.empty()?, &Limits(32, 1, 2, 8)))?
shown(requests_from(std.bytes.empty()?, &Limits(32, 64, -1, 8)))?
shown(requests_from(std.bytes.empty()?, &Limits(32, 64, 2, -1)))?
"#,
        r#"
err LineTooLong
err HeadTooLarge
err HeadTooLarge
err TooManyHeaders
req GET / 1 2 Empty True ""
eof
err BodyTooLarge
err BodyTooLarge
req POST / 1 1 Chunked True "12345678"
eof
err TooManyHeaders
err BodyTooLarge
err InvalidLimit
err InvalidLimit
err InvalidLimit
err InvalidLimit
"#,
    );
}

#[test]
fn writers_build_heads_and_reject_injection() {
    native(
        r#"
mut get = std.http.request("GET", "/search?q=a%20b").unwrap()
get.add("Host", "example.com").unwrap()
get.add("Accept", "text/plain, */*").unwrap()
mut sink = std.io.cursor(std.bytes.empty()?)
written(std.http.write_request(&mut sink, &get), &sink)?
mut created = std.http.response(201, "Created").unwrap()
created.add("Content-Length", "0").unwrap()
mut out = std.io.cursor(std.bytes.empty()?)
written(std.http.write_response(&mut out, &created), &out)?
mut legacy = std.http.response(200, "").unwrap()
legacy.minor = 0
mut old = std.io.cursor(std.bytes.empty()?)
written(std.http.write_response(&mut old, &legacy), &old)?
mut bad = std.io.cursor(std.bytes.empty()?)
written(std.http.write_request(&mut bad, &std.http.request("GE T", "/").unwrap()), &bad)?
written(std.http.write_request(&mut bad, &std.http.request("GET", "/a b").unwrap()), &bad)?
written(std.http.write_request(&mut bad, &std.http.request("GET", "").unwrap()), &bad)?
mut split = std.http.request("GET", "/").unwrap()
split.add("X", "a\r\nInjected: 1").unwrap()
written(std.http.write_request(&mut bad, &split), &bad)?
mut padded = std.http.request("GET", "/").unwrap()
padded.add("X", " a").unwrap()
written(std.http.write_request(&mut bad, &padded), &bad)?
mut named = std.http.request("GET", "/").unwrap()
named.add("X:Y", "a").unwrap()
written(std.http.write_request(&mut bad, &named), &bad)?
mut both = std.http.request("POST", "/").unwrap()
both.add("Content-Length", "1").unwrap()
both.add("Transfer-Encoding", "chunked").unwrap()
written(std.http.write_request(&mut bad, &both), &bad)?
mut chunked10 = std.http.request("POST", "/").unwrap()
chunked10.minor = 0
chunked10.add("Transfer-Encoding", "chunked").unwrap()
written(std.http.write_request(&mut bad, &chunked10), &bad)?
written(std.http.write_response(&mut bad, &std.http.response(99, "x").unwrap()), &bad)?
written(std.http.write_response(&mut bad, &std.http.response(200, "a\nb").unwrap()), &bad)?
print(bad.data.len())?
"#,
        r#"
"GET /search?q=a%20b HTTP/1.1\r\nHost: example.com\r\nAccept: text/plain, */*\r\n\r\n"
"HTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n"
"HTTP/1.0 200 \r\n\r\n"
err InvalidRequestLine
err InvalidRequestLine
err InvalidRequestLine
err InvalidHeader
err InvalidHeader
err InvalidHeader
err ConflictingFraming
err UnsupportedTransferEncoding
err InvalidStatusLine
err InvalidStatusLine
0
"#,
    );
}

#[test]
fn written_bodies_read_back_with_the_same_framing() {
    native(
        r#"
piece = std.bytes.from_text("hello, world")?
mut post = std.http.request("POST", "/upload").unwrap()
post.add("Transfer-Encoding", "chunked").unwrap()
mut sink = std.io.cursor(std.bytes.empty()?)
std.http.write_request(&mut sink, &post).unwrap()
std.http.write_chunk(&mut sink, &piece, 0, 5).unwrap()
std.http.write_chunk(&mut sink, &piece, 5, 5).unwrap()
std.http.write_chunk(&mut sink, &piece, 5, 12).unwrap()
mut trailers = list[Header].new()?
trailers.append(Header("Digest", "abc"))?
std.http.write_last_chunk(&mut sink, &trailers).unwrap()
mut put = std.http.request("PUT", "/x").unwrap()
put.add("Content-Length", "12").unwrap()
std.http.write_request(&mut sink, &put).unwrap()
print(str.repr(std.http.write_body(&mut sink, &Framing.Length(12), &piece, 0, 11))?)?
print(str.repr(std.http.write_body(&mut sink, &Framing.Empty, &piece, 0, 1))?)?
print(str.repr(std.http.write_chunk(&mut sink, &piece, 3, 13))?)?
std.http.write_body(&mut sink, &Framing.Length(12), &piece, 0, 12).unwrap()
mut short = std.http.request("PATCH", "/y").unwrap()
short.add("Transfer-Encoding", "chunked").unwrap()
std.http.write_request(&mut sink, &short).unwrap()
std.http.write_body(&mut sink, &Framing.Chunked, &piece, 7, 12).unwrap()
mut bad_trailer = list[Header].new()?
bad_trailer.append(Header("A", "\r\n"))?
print(str.repr(std.http.write_last_chunk(&mut sink, &bad_trailer))?)?
shown(requests_from(replace(sink.data, std.bytes.empty()?), &std.http.limits()))?
"#,
        r#"
Result[(), std.http.HttpError].Err(LengthMismatch)
Result[(), std.http.HttpError].Err(LengthMismatch)
Result[(), std.http.HttpError].Err(Stream(OutOfRange))
Result[(), std.http.HttpError].Err(InvalidHeader)
req POST /upload 1 1 Chunked True "hello, world Digest=abc"
req PUT /x 1 1 Length(12) True "hello, world"
req PATCH /y 1 1 Chunked True "world"
eof
"#,
    );
}

/// A reader that hands out one byte per call and reports `Interrupted`
/// before every byte, as a slow peer or a signal would.
const TRICKLE: &str = r#"
pub class Trickle:
    pub data: Bytes
    pub position: i64
    pub pending: bool

    pub def read(self: &mut Trickle, buffer: &mut Bytes, start: i64, stop: i64) -> Result[i64, StreamError]:
        if start == stop or self.position == self.data.len():
            return Ok(0)
        self.pending = not self.pending
        if self.pending:
            return Err(StreamError.Interrupted)
        buffer[start] = self.data[self.position]
        self.position += 1
        Ok(1)
"#;

#[test]
fn a_trickling_interrupted_peer_reads_like_a_whole_buffer() {
    let source = program(
        r#"
text = "POST /t HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2;x=y\r\nde\r\n0\r\nT: 1\r\n\r\nGET /next HTTP/1.1\r\nContent-Length: 2\r\n\r\nhi"
mut conn = std.io.buffered_reader(Trickle(std.bytes.from_text(text)?, 0, False), 3)?
limits = std.http.limits()
mut seen = 0
while True:
    match std.http.read_request(&mut conn, &limits).unwrap():
        case Nothing:
            break
        case Some(head):
            framing = std.http.request_framing(&head).unwrap()
            mut body = std.http.body_reader(conn, &framing, &limits).unwrap()
            print(head.target, body_text(&mut body).unwrap())?
            conn = std.http.finish(&mut body).unwrap()
            seen += 1
print(seen)?
"#,
    )
    .replacen("\ndef main", &format!("{TRICKLE}\ndef main"), 1);
    assert_eq!(output(&source), "/t abcde T=1\n/next hi\n2");
}

#[test]
fn a_body_is_a_std_io_reader() {
    native(
        r#"
raw = std.bytes.from_text("4\r\nabcd\r\n3\r\nefg\r\n0\r\n\r\nrest")?
mut body = std.http.body_reader(std.io.buffered_reader(std.io.cursor(raw), 64)?, &Framing.Chunked, &std.http.limits()).unwrap()
mut target = std.io.cursor(std.bytes.empty()?)
mut scratch = std.bytes.with_capacity(3)?
scratch.resize(3, 0u8)?
print(str.repr(std.io.copy_all(&mut body, &mut target, &mut scratch))?, target.data.to_text()?, body.total(), body.is_done())?
mut conn = std.http.finish(&mut body).unwrap()
mut rest = std.io.cursor(std.bytes.empty()?)
std.io.copy_all(&mut conn, &mut rest, &mut scratch).unwrap()
print(rest.data.to_text()?)?
bad = std.bytes.from_text("2\r\nab\r\nzz\r\n")?
mut broken = std.http.body_reader(std.io.buffered_reader(std.io.cursor(bad), 64)?, &Framing.Chunked, &std.http.limits()).unwrap()
mut out = std.io.cursor(std.bytes.empty()?)
print(str.repr(std.io.copy_all(&mut broken, &mut out, &mut scratch))?)?
print(str.repr(broken.take_failure())?, str.repr(broken.take_failure())?)?
print(str.repr(broken.read_body(&mut scratch, 0, 3))?)?
print(str.repr(std.http.finish(&mut broken).is_ok())?)?
print(std.http.release(&mut broken).is_some(), std.http.release(&mut broken).is_some())?
short = std.bytes.from_text("ab")?
mut cut = std.http.body_reader(std.io.buffered_reader(std.io.cursor(short), 64)?, &Framing.Length(3), &std.http.limits()).unwrap()
print(str.repr(std.io.copy_all(&mut cut, &mut out, &mut scratch))?)?
unread = std.bytes.from_text("abc")?
mut pending = std.http.body_reader(std.io.buffered_reader(std.io.cursor(unread), 64)?, &Framing.Length(3), &std.http.limits()).unwrap()
print(str.repr(std.http.finish(&mut pending).is_ok())?, std.http.discard(&mut pending).unwrap(), std.http.finish(&mut pending).is_ok())?
"#,
        r#"
Result[i64, std.io.CopyError].Ok(7) abcdefg 7 True
rest
Result[i64, std.io.CopyError].Err(std.io.CopyError.Read(2, Io(IoError.InvalidInput)))
Option[std.http.HttpError].Some(InvalidChunk) Option[std.http.HttpError].Nothing
Result[i64, std.http.HttpError].Err(Broken)
False
True False
Result[i64, std.io.CopyError].Err(std.io.CopyError.Read(2, UnexpectedEof))
False 3 True
"#,
    );
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
}

/// Retained corpus: valid messages whose mutations reach every parser state.
const CORPUS: &[&[u8]] = &[
    b"GET /a HTTP/1.1\r\nHost: x\r\n\r\nGET /b HTTP/1.1\r\n\r\n",
    b"POST / HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello",
    b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n3;e=v\r\nabc\r\nA\r\n0123456789\r\n0\r\nT: 1\r\n\r\n",
    b"\r\nGET / HTTP/1.0\r\nConnection: keep-alive\r\n\r\n",
];

/// Mutated input must end in a message, `eof`, or a protocol error: never a
/// trap, a hang, or an allocation failure from a runaway size.
#[test]
fn mutated_messages_fail_cleanly() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let alphabet = b"\r\n :;,0123456789aAfFzZ-\t\x00\x7f\x80";
    let mut inputs: Vec<Vec<u8>> = Vec::new();
    for _ in 0..400 {
        let mut input = CORPUS[(rng.next() % CORPUS.len() as u64) as usize].to_vec();
        for _ in 0..1 + rng.next() % 3 {
            let at = (rng.next() % input.len() as u64) as usize;
            match rng.next() % 3 {
                0 => input[at] = alphabet[(rng.next() % alphabet.len() as u64) as usize],
                1 => {
                    input.remove(at);
                }
                _ => input.insert(at, alphabet[(rng.next() % alphabet.len() as u64) as usize]),
            }
            if input.is_empty() {
                input.push(b'G');
            }
        }
        inputs.push(input);
    }
    let refs: Vec<&[u8]> = inputs.iter().map(Vec::as_slice).collect();
    for (input, result) in refs.iter().zip(each("requests_from", &refs)) {
        let last = result.lines().last().unwrap_or("");
        assert!(
            last == "eof" || (last.starts_with("err ") && !last.contains("Allocation")),
            "{:?} -> {result}",
            String::from_utf8_lossy(input)
        );
    }
}

#[test]
fn a_loopback_server_reuses_one_connection() {
    let source = format!(
        r#"{HEADER}{HELPERS}
from std.net import NetError, Shutdown, TcpListener, TcpStream, connect, listen, loopback_v4
from std.sync import CancellationToken
from std.time import Deadline, deadline_after, milliseconds, monotonic_now

# Serves requests on one connection until the client asks to close; each
# reply echoes the request body with Content-Length framing.
def serve(listener: &TcpListener, deadline: &Deadline, cancel: &CancellationToken) -> Result[i64, HttpError]:
    stream = listener.accept(deadline, cancel).unwrap()
    limits = std.http.limits()
    mut conn = std.io.buffered_reader(stream, 64).unwrap()
    mut served = 0
    while True:
        match std.http.read_request(&mut conn, &limits)?:
            case Nothing:
                break
            case Some(head):
                framing = std.http.request_framing(&head)?
                mut body = std.http.body_reader(conn, &framing, &limits)?
                mut echoed = std.bytes.empty().unwrap()
                std.http.read_to_end(&mut body, &mut echoed)?
                conn = std.http.finish(&mut body)?
                mut reply = std.http.response(200, "OK")?
                reply.add("Content-Length", str.from(echoed.len()).unwrap())?
                keep = std.http.request_keeps_alive(&head)
                if not keep:
                    reply.add("Connection", "close")?
                std.http.write_response(&mut conn.inner, &reply)?
                std.http.write_body(&mut conn.inner, &Framing.Length(echoed.len()), &echoed, 0, echoed.len())?
                served += 1
                if not keep:
                    break
    conn.inner.shutdown(Shutdown.Write).unwrap()
    Ok(served)

def main() -> Result[(), std.bytes.BytesError | RangeError | std.io.StreamError | ThreadError | AllocError | IoError]:
    cancel = CancellationToken()?
    deadline = deadline_after(&monotonic_now(), &milliseconds(5000u32))?
    listener = listen(&loopback_v4(0u16), 4).unwrap()
    address = listener.local_address().unwrap()
    limits = std.http.limits()
    with spawn(serve, &listener, &deadline, &cancel)? as task:
        stream = connect(&address, &deadline, &cancel).unwrap()
        mut conn = std.io.buffered_reader(stream, 64)?
        for index in range(3):
            mut post = std.http.request("POST", "/echo").unwrap()
            post.add("Host", "localhost").unwrap()
            post.add("Transfer-Encoding", "chunked").unwrap()
            if index == 2:
                post.add("Connection", "close").unwrap()
            payload = std.bytes.from_text("message ".concat(str.from(index)?)?)?
            std.http.write_request(&mut conn.inner, &post).unwrap()
            std.http.write_chunk(&mut conn.inner, &payload, 0, 3).unwrap()
            std.http.write_chunk(&mut conn.inner, &payload, 3, payload.len()).unwrap()
            std.http.write_last_chunk(&mut conn.inner, &list[Header].new()?).unwrap()
            head = std.http.read_response(&mut conn, &limits).unwrap().unwrap()
            framing = std.http.response_framing(&head, "POST").unwrap()
            mut body = std.http.body_reader(conn, &framing, &limits).unwrap()
            print(head.status, std.http.response_keeps_alive(&head, &framing), body_text(&mut body).unwrap())?
            conn = std.http.finish(&mut body).unwrap()
        print(str.repr(std.http.read_response(&mut conn, &limits).unwrap().is_some())?)?
        print(task.join().unwrap())?
    Ok(())
"#
    );
    assert_eq!(
        output(&source),
        "200 True message 0\n200 True message 1\n200 False message 2\nFalse\n3"
    );
}

/// Body reads have no deadline of their own: the caller supplies a `Reader`
/// that waits with one. Cancelling the token stops a server waiting on a
/// client that sent only part of its body.
#[test]
fn a_cancelled_read_stops_a_partial_body() {
    let source = format!(
        r#"{HEADER}{HELPERS}
from std.net import NetError, TcpListener, TcpStream, connect, listen, loopback_v4
from std.sync import CancellationToken
from std.time import Deadline, deadline_after, milliseconds, monotonic_now

pub class Bounded:
    pub stream: TcpStream
    pub deadline: Deadline
    pub cancel: CancellationToken
    pub failure: Option[NetError]

    pub def read(self: &mut Bounded, buffer: &mut Bytes, start: i64, stop: i64) -> Result[i64, StreamError]:
        match self.stream.read_before(buffer, start, stop, &self.deadline, &self.cancel):
            case Ok(count):
                Ok(count)
            case Err(error):
                self.failure = Some(error)
                Err(StreamError.Io(IoError.Other))

def serve(listener: &TcpListener, deadline: &Deadline, cancel: &CancellationToken) -> Result[str, std.bytes.BytesError | RangeError | std.io.StreamError | AllocError]:
    stream = listener.accept(deadline, &CancellationToken()?).unwrap()
    limits = std.http.limits()
    within = deadline_after(&monotonic_now(), &milliseconds(5000u32))?
    mut conn = std.io.buffered_reader(Bounded(stream, within, cancel.share(), Nothing), 64)?
    head = std.http.read_request(&mut conn, &limits).unwrap().unwrap()
    framing = std.http.request_framing(&head).unwrap()
    mut interim = std.io.cursor(std.bytes.empty()?)
    std.http.write_response(&mut interim, &std.http.response(100, "Continue").unwrap()).unwrap()
    conn.inner.stream.write_all_before(&interim.data, 0, interim.data.len(), deadline, &CancellationToken()?).unwrap()
    mut body = std.http.body_reader(conn, &framing, &limits).unwrap()
    mut out = std.bytes.empty()?
    outcome = str.repr(std.http.read_to_end(&mut body, &mut out))?
    mut kept = std.http.release(&mut body).unwrap()
    Ok(outcome.concat(" ")?.concat(&str.repr(&kept.inner.failure)?)?.concat(" ")?.concat(&str.from(out.len())?)?)

def main() -> Result[(), std.bytes.BytesError | RangeError | std.io.StreamError | ThreadError | AllocError | IoError]:
    cancel = CancellationToken()?
    deadline = deadline_after(&monotonic_now(), &milliseconds(5000u32))?
    listener = listen(&loopback_v4(0u16), 4).unwrap()
    address = listener.local_address().unwrap()
    with spawn(serve, &listener, &deadline, &cancel)? as task, cancel.cancel_on_exit():
        stream = connect(&address, &deadline, &cancel).unwrap()
        mut conn = std.io.buffered_reader(stream, 64)?
        start = std.bytes.from_text("PUT /slow HTTP/1.1\r\nContent-Length: 10\r\n\r\nabc")?
        conn.inner.write_all_before(&start, 0, start.len(), &deadline, &cancel).unwrap()
        interim = std.http.read_response(&mut conn, &std.http.limits()).unwrap().unwrap()
        print(interim.status)?
        cancel.cancel()
        print(task.join().unwrap())?
    Ok(())
"#
    );
    assert_eq!(
        output(&source),
        "100\nResult[i64, std.http.HttpError].Err(Stream(Io(IoError.Other))) Option[std.net.NetError].Some(std.net.NetError.Cancelled) 3"
    );
}

#[cfg(feature = "runtime-checks")]
fn fails_cleanly_at_every_allocation(setup: &str, call: &str) {
    let attempts = 120;
    let mut body = String::from(setup.trim_matches('\n'));
    body.push('\n');
    for attempt in 0..attempts {
        body.push_str(&format!(
            "print(\"__test_fail_allocations_after_{attempt}__\")?\n\
             r{attempt} = {call}\n\
             print(\"__test_restore_allocations__\")?\n\
             print(tag(&r{attempt}))?\n"
        ));
    }
    let result = output(&program(&body));
    let tags: Vec<&str> = result.lines().collect();
    assert_eq!(tags.len(), attempts, "{call}");
    let failures = tags.iter().take_while(|tag| **tag == "alloc").count();
    assert!(failures > 0 && failures < attempts, "{call}: {tags:?}");
    assert!(
        tags[failures..].iter().all(|tag| *tag == "ok"),
        "{call}: {tags:?}"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_is_reported_at_every_point_without_leaking() {
    let setup = r#"
text = "POST /a-target-too-long-for-inline-string-storage HTTP/1.1\r\nA-Header-Name-Too-Long-For-Inline-Storage: a-value-too-long-for-inline-string-storage\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\nA-Trailer-Name-Too-Long-For-Inline-Storage: x\r\n\r\n"
raw = std.bytes.from_text(text)?
mut reader = std.io.buffered_reader(std.io.cursor(raw.copy()?), 16)?
head = std.http.read_request(&mut reader, &std.http.limits()).unwrap().unwrap()
mut sink = std.io.cursor(std.bytes.with_capacity(4096)?)
"#;
    for call in ["drain(&raw)", "std.http.write_request(&mut sink, &head)"] {
        fails_cleanly_at_every_allocation(setup, call);
    }
}

/// Issue #271: `same_name` compares header names without allocating, and
/// `std.http.cookie.cookies` uses it to find `Cookie` fields in any case.
#[cfg(feature = "runtime-checks")]
#[test]
fn header_names_compare_without_allocating() {
    let body = r#"
long = "A-Header-Name-Too-Long-For-Inline-Storage"
print("__test_begin_no_allocations__")?
same = std.http.same_name(long, "a-header-name-too-long-for-inline-storage")
shorter = std.http.same_name(long, "a-header-name")
other = std.http.same_name("Cookie", "cookif")
print("__test_end_no_allocations__")?
print(same, shorter, other)?
headers = [Header(long, "x"), Header("COOKIE", "a=1; b=2"), Header("cookie", "c=3")]?
for cookie in std.http.cookie.cookies(&headers, 8).unwrap():
    print(cookie.name, cookie.value)?
"#;
    assert_eq!(
        output(&format!("import std.http.cookie\n{}", program(body))),
        "True False False\na 1\nb 2\nc 3"
    );
}

/// Issue #135: the authority shares the Host value's storage instead of
/// copying it, so `request_target` for an origin-form `/` allocates nothing,
/// and the authority outlives the head it came from.
#[cfg(feature = "runtime-checks")]
#[test]
fn request_target_shares_the_host_value() {
    native(
        r#"
text = "GET / HTTP/1.1\r\nHost: a-host-name-too-long-for-inline-storage.example:8080\r\n\r\n"
mut reader = std.io.buffered_reader(std.io.cursor(std.bytes.from_text(text)?), 64)?
mut authority = ""
if True:
    head = std.http.read_request(&mut reader, &std.http.limits()).unwrap().unwrap()
    print("__test_begin_no_allocations__")?
    target = std.http.request_target(&head).unwrap()
    print("__test_end_no_allocations__")?
    authority = target.authority
print(authority)?
"#,
        "a-host-name-too-long-for-inline-storage.example:8080",
    );
}
