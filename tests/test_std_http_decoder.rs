//! Readiness-driven HTTP decoding retains the stream parser's wire rules.
mod support;

const IMPORTS: &str = r#"
import std.bytes
import std.http
import std.http.decoder
import std.io
import std.test
from std.bytes import Bytes
from std.http import HttpError
from std.http.decoder import Event

def ensure(condition: bool) -> ():
    std.test.check(condition, "decoder contract").unwrap()
"#;

#[test]
fn fragmented_heads_bodies_trailers_and_pipelines() {
    support::assert_stdout(&format!(
        r#"{IMPORTS}
def run(step: i64) -> ():
    input = std.bytes.from_text("\r\nPOST http://example.test/a?q=1 HTTP/1.1\r\nHost: ignored\r\nTransfer-Encoding: chunked\r\n\r\n3;ext=yes\r\nabc\r\n2\r\nde\r\n0\r\nX-End: done\r\n\r\nGET / HTTP/1.0\r\n\r\n").unwrap()
    mut parser = std.http.decoder.decoder(&std.http.limits()).unwrap()
    mut at = 0
    mut bodies = std.bytes.empty().unwrap()
    mut ends = 0
    while at < input.len() or parser.has_head():
        mut stop = at + step
        if stop > input.len():
            stop = input.len()
        consumed, event = parser.feed(&input[at:stop]).unwrap()
        ensure(consumed <= std.http.decoder.feed_budget)
        at += consumed
        match event:
            case Some(Event.Head(request, target, framing)):
                print(request.method, target.authority, target.path, target.query, framing).unwrap()
            case Some(Event.Body(data)):
                bodies.extend(&data).unwrap()
            case Some(Event.Trailers(fields)):
                print(fields).unwrap()
            case Some(Event.End):
                ends += 1
                paused, nothing = parser.feed(&input[at:]).unwrap()
                ensure(paused == 0)
                ensure(nothing.is_none())
                parser.reset().unwrap()
            case Nothing:
                pass
    parser.finish().unwrap()
    print(bodies.to_text().unwrap(), ends).unwrap()

def main() -> ():
    run(1)
    run(7)
    run(65536)
"#
    ), &"POST example.test /a q=1 std.http.Framing.Chunked\n[std.http.Header(name=\"X-End\", value=\"done\")]\nGET  /  std.http.Framing.Empty\nabcde 2\n".repeat(3));
}

#[test]
fn empty_feed_is_not_eof_and_finished_requests_require_reset() {
    support::assert_stdout(
        &format!(
            r#"{IMPORTS}
def main() -> ():
    mut parser = std.http.decoder.decoder(&std.http.limits()).unwrap()
    blank = std.bytes.from_text("\r\n").unwrap()
    drop(parser.feed(&blank).unwrap())
    ensure(not parser.is_started())
    parser.finish().unwrap()
    ensure(parser.reset().is_err())
    data = std.bytes.from_text("GET / HTTP/1.1\r\nHost: x\r\n\r\n").unwrap()
    drop(parser.feed(&data[:4]).unwrap())
    ensure(parser.is_started())
    consumed, event = parser.feed(&data[4:4]).unwrap()
    ensure(consumed == 0 and event.is_none())
    print(str.repr(parser.finish()).unwrap()).unwrap()
    print(str.repr(parser.feed(&data[4:])).unwrap()).unwrap()
"#
        ),
        "Result[(), std.http.HttpError].Err(std.http.HttpError.Truncated)\nResult[tuple[i64, Option[std.http.decoder.Event]], std.http.HttpError].Err(std.http.HttpError.Broken)\n",
    );
}
const PARITY: &str = r#"
import std.encoding

def reference(input: &Bytes, limits: &std.http.Limits) -> Result[(i64, i64), HttpError]:
    mut reader = std.io.buffered_reader(std.io.cursor(input.copy().unwrap()), 7).unwrap()
    mut requests = 0
    mut total = 0
    while True:
        match std.http.read_request(&mut reader, limits)?:
            case Nothing:
                return Ok((requests, total))
            case Some(request):
                framing = std.http.request_framing(&request)?
                # The HTTP server applies this bound before routing.
                match &framing:
                    case std.http.Framing.Length(size):
                        if *size > limits.max_body:
                            return Err(HttpError.BodyTooLarge)
                    case _:
                        pass
                drop(std.http.request_target(&request)?)
                mut body = std.http.body_reader(reader, &framing, limits)?
                total += std.http.discard(&mut body)?
                requests += 1
                reader = std.http.finish(&mut body)?
    Ok((requests, total))

def incremental(input: &Bytes, limits: &std.http.Limits, step: i64) -> Result[(i64, i64), HttpError]:
    mut parser = std.http.decoder.decoder(limits)?
    mut at = 0
    mut requests = 0
    mut total = 0
    while at < input.len() or parser.has_head():
        mut stop = at + step
        if stop > input.len():
            stop = input.len()
        consumed, event = parser.feed(&input[at:stop])?
        ensure(consumed <= std.http.decoder.feed_budget)
        at += consumed
        mut emitted = False
        match event:
            case Some(Event.Body(body)):
                total += body.len()
                emitted = True
            case Some(Event.End):
                requests += 1
                parser.reset()?
                emitted = True
            case Some(_):
                emitted = True
            case Nothing:
                pass
        if consumed == 0 and not emitted:
            break
    parser.finish()?
    Ok((requests, total))

def compare(encoded: &str, limits: &std.http.Limits) -> ():
    input = std.encoding.hex_from_text(encoded).unwrap()
    baseline = reference(&input, limits)
    for step in [1, 7, 4096, 65536].unwrap():
        actual = incremental(&input, limits, step)
        if actual != baseline:
            print(encoded, step, str.repr(&baseline).unwrap(), str.repr(&actual).unwrap()).unwrap()
            ensure(False)
"#;

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

#[test]
fn retained_server_corpus_has_stream_parser_parity_at_every_fragment_size() {
    let calls: String = include_str!("fixtures/http_server/corpus.txt")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let (_, raw) = line.split_once(" |").expect("case separator");
            let data = unescape(raw.strip_prefix(' ').unwrap_or(raw));
            format!("    compare(\"{}\", &limits)\n", support::hex(&data))
        })
        .collect();
    support::assert_stdout(
        &format!(
            "{IMPORTS}{PARITY}\ndef main() -> ():\n    limits = std.http.limits()\n{calls}    print(\"ok\").unwrap()\n"
        ),
        "ok\n",
    );
}

#[test]
fn exact_limits_chunk_extensions_overflow_and_truncated_prefixes() {
    let mut calls = String::new();
    for input in [
        "POST / HTTP/1.1\r\nHost: x\r\nContent-Length: 2\r\n\r\nab",
        "POST / HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n2;hello=world\r\nab\r\n0\r\nT: x\r\n\r\n",
        "POST / HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n7fffffffffffffff\r\n",
        "POST / HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n8000000000000000\r\n",
        "POST / HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n1 \r\na\r\n0\r\n\r\n",
        "\r\n\r\n",
        "\r\r\n",
    ] {
        for length in 0..=input.len() {
            calls.push_str(&format!(
                "    compare(\"{}\", &std.http.limits())\n",
                support::hex(&input.as_bytes()[..length])
            ));
        }
    }
    for limits in [
        "2, 2, 0, 0",
        "16, 64, 2, 2",
        "17, 64, 2, 2",
        "8192, 27, 1, 2",
        "8192, 28, 1, 2",
        "8192, 65536, 0, 0",
        "8192, 65536, 1, 0",
    ] {
        for input in [
            "\r\n",
            "GET / HTTP/1.1\r\nHost: x\r\n\r\n",
            "POST / HTTP/1.1\r\nHost: x\r\nContent-Length: 3\r\n\r\nabc",
            "GET / HTTP/1.0\r\n\r\n",
        ] {
            calls.push_str(&format!(
                "    compare(\"{}\", &std.http.Limits({limits}))\n",
                support::hex(input.as_bytes())
            ));
        }
    }
    let lines: Vec<_> = calls.lines().collect();
    let mut functions = String::new();
    let mut main = String::new();
    for (index, batch) in lines.chunks(64).enumerate() {
        functions.push_str(&format!(
            "def batch_{index}() -> ():\n{}\n\n",
            batch.join("\n")
        ));
        main.push_str(&format!("    batch_{index}()\n"));
    }
    support::assert_stdout(
        &format!(
            "{IMPORTS}{PARITY}\n{functions}def main() -> ():\n{main}    print(\"ok\").unwrap()\n"
        ),
        "ok\n",
    );
}

#[test]
fn decoder_state_can_be_stored_in_an_actor_node() {
    support::assert_stdout(
        &format!(
            r#"{IMPORTS}
import std.actor
import std.sync
import std.time
from std.actor import Address, Context, Mail

pub class Node:
    pub parser: Option[std.http.decoder.Decoder]

    pub def started(self: &mut Node, context: &mut Context[i64, HttpError]) -> Result[(), HttpError]:
        self.parser = Some(std.http.decoder.decoder(&std.http.limits())?)
        context.stop()
        Ok(())

    pub def receive(self: &mut Node, mail: Mail[i64, HttpError], context: &mut Context[i64, HttpError]) -> Result[(), HttpError]:
        Ok(())

    pub def spawn(self, tag: i64, child: Address, context: &mut Context[i64, HttpError]) -> Result[Option[Node], HttpError]:
        Ok(Nothing)

def main() -> ():
    mut actors = std.actor.stage[Node, i64, HttpError](1, 1, 1).unwrap()
    actors.start(Node(Nothing), 65536).unwrap()
    actors.run(&std.time.deadline_after(&std.time.monotonic_now(), &std.time.seconds(2u32)).unwrap(), &std.sync.CancellationToken().unwrap()).unwrap()
    print(std.actor.heaps_live(), std.actor.heap_bytes_used()).unwrap()
"#
        ),
        "0 0\n",
    );
}

#[test]
fn literal_and_owned_views_share_the_decoder_contract() {
    support::assert_stdout(
        &format!(
            r#"{IMPORTS}
def run(source: &[u8]) -> ():
    mut parser = std.http.decoder.decoder(&std.http.limits()).unwrap()
    ensure(not parser.is_started() and not parser.has_head() and not parser.is_done())
    mut at = 0
    mut body = std.bytes.empty().unwrap()
    while not parser.is_done():
        consumed, event = parser.feed(&source[at:]).unwrap()
        at += consumed
        match event:
            case Some(Event.Head(_, _, _)):
                ensure(parser.is_started() and parser.has_head())
            case Some(Event.Body(part)):
                body.extend(&part).unwrap()
            case _:
                pass
    ensure(at == len(source))
    print(body.to_text().unwrap()).unwrap()
    parser.reset().unwrap()
    ensure(not parser.is_started() and not parser.has_head() and not parser.is_done())
    ensure(not parser.is_started())

def main() -> ():
    literal = b"POST / HTTP/1.1\r\nHost: x\r\nContent-Length: 3\r\n\r\nabc"
    run(&literal)
    owned = std.bytes.from_bytes(&literal).unwrap()
    run(&owned)
    ensure(std.http.parse_chunk_size(b"a;ext=yes").unwrap() == 10)
    ensure(std.http.parse_chunk_size(b"a;ext=\x7f").is_err())
    ensure(std.http.parse_chunk_size(b"").is_err())
"#
        ),
        "abc\nabc\n",
    );
}
