//! `std.json`: the resumable event decoder, the event encoder, and the value
//! tree, checked against hand-written expectations and an independent
//! reference parser.
mod support;
use support::hex;

use std::collections::HashSet;

/// Helpers shared by every program. Inputs arrive as hex so a case can hold
/// any byte sequence. `check` prints the whole-buffer outcome and reports any
/// short-read stream that disagrees with it.
const HEADER: &str = r#"import std.bytes
import std.encoding
import std.io
import std.json
from std.bytes import ByteSource, Bytes
from std.io import StreamError
from std.json import DuplicateKeys, Event, Json, JsonError, JsonFault, Limits, Member

pub class Trickle:
    pub data: Bytes
    pub at: i64
    pub step: i64

    pub def read(self: &mut Trickle, buffer: &mut Bytes, start: i64, stop: i64) -> Result[i64, StreamError]:
        mut count = stop - start
        if count > self.step:
            count = self.step
        if count > self.data.len() - self.at:
            count = self.data.len() - self.at
        match buffer.overwrite(start, &self.data, self.at, self.at + count):
            case Ok(_):
                pass
            case Err(_):
                return Err(StreamError.OutOfRange)
        self.at += count
        Ok(count)

def unhex(digits: &str) -> Result[Bytes, AllocError | IoError | std.encoding.EncodingError]:
    Ok(std.encoding.hex_from_text(digits)?)

def placed(error: &JsonError) -> Result[str, AllocError | IoError]:
    Ok(" ".join(&[str.repr(&error.fault)?, str.from(error.offset)?, str.from(error.line)?, str.from(error.column)?]?)?)

def wide() -> Limits:
    mut bounds = std.json.limits()
    bounds.max_depth = 1024
    bounds

def described(result: Result[Json, JsonError]) -> Result[str, AllocError | IoError | std.json.JsonError]:
    match result:
        case Ok(value):
            bounds = wide()
            Ok(std.json.encode_text(&value, &bounds)?)
        case Err(error):
            placed(&error)

def settled(result: Result[(), JsonError]) -> Result[str, AllocError | IoError]:
    match result:
        case Ok(_):
            Ok("ok")
        case Err(error):
            placed(&error)

def trickled(source: &Bytes, bounds: &Limits, step: i64) -> Result[str, AllocError | IoError | std.bytes.BytesError | std.json.JsonError]:
    mut reader = Trickle(source.copy()?, 0, step)
    described(std.json.read_value(&mut reader, bounds))

def check(digits: &str, bounds: &Limits) -> Result[(), AllocError | IoError | std.encoding.EncodingError | std.json.JsonError | std.bytes.BytesError]:
    source = unhex(digits)?
    whole = described(std.json.parse(&source, bounds))?
    one = trickled(&source, bounds, 1)?
    if one != whole:
        print("STREAM-1", one)?
    three = trickled(&source, bounds, 3)?
    if three != whole:
        print("STREAM-3", three)?
    print(whole)?
    Ok(())

def waiting(event: &Event) -> bool:
    match event:
        case Event.Pending:
            True
        case Event.End:
            True
        case _:
            False

# Appends the name of every event available without more input.
def drain(decoder: &mut std.json.Decoder, names: &mut list[str]) -> Result[(), AllocError | IoError]:
    while True:
        outcome = decoder.next()
        match outcome:
            case Ok(event):
                if waiting(&event):
                    break
                names.append(str.repr(&event)?)?
            case Err(error):
                names.append(placed(&error)?)?
                break
    Ok(())

# The events of `source` when it arrives as [0, split) and then the rest.
def events(source: &Bytes, bounds: &Limits, split: i64) -> Result[str, AllocError | IoError | std.json.JsonError]:
    mut decoder = std.json.decoder(bounds)?
    mut names = list[str]()?
    decoder.feed(source, 0, split)?
    drain(&mut decoder, &mut names)?
    drop(decoder.feed(source, split, source.len()))
    decoder.finish()
    drain(&mut decoder, &mut names)?
    Ok(" ".join(&names)?)
"#;

fn program(body: &str) -> String {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    format!("{HEADER}\ndef main() -> Result[(), AllocError | IoError | std.bytes.BytesError | std.encoding.EncodingError | std.json.JsonError | ParseError | std.io.StreamError]:\n{body}    Ok(())\n")
}

/// Runs a program and shortens the qualified names in rendered values.
fn output(source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("std.json.JsonFault.", "")
        .replace("std.json.Event.", "")
        .replace("std.io.StreamError.", "")
}

fn native(body: &str, expected: &str) {
    assert_eq!(
        output(&program(body)),
        expected.trim_matches('\n'),
        "{body}"
    );
}

/// One `check` outcome per input, under limits prepared by `setup`.
fn outcomes(inputs: &[Vec<u8>], setup: &str) -> Vec<String> {
    let cases = inputs
        .iter()
        .map(|input| format!("\"{}\"", hex(input)))
        .collect::<Vec<_>>()
        .join(", ");
    let body = format!(
        "mut bounds = std.json.limits()\n{setup}\ncases = [{cases}]?\nfor digits in &cases:\n    check(digits, &bounds)?\n"
    );
    let text = output(&program(&body));
    let lines = text.lines().map(str::to_string).collect::<Vec<_>>();
    assert_eq!(lines.len(), inputs.len(), "{text}");
    lines
}

fn table(cases: &[(&[u8], &str)], setup: &str) {
    let inputs = cases
        .iter()
        .map(|(input, _)| input.to_vec())
        .collect::<Vec<_>>();
    let actual = outcomes(&inputs, setup);
    for ((input, expected), actual) in cases.iter().zip(&actual) {
        assert_eq!(actual, expected, "{}", String::from_utf8_lossy(input));
    }
}

/// An independent recursive-descent parser. `Some` holds the canonical
/// compact encoding; `None` means the document must be rejected.
struct Reference<'a> {
    input: &'a [u8],
    at: usize,
    max_depth: usize,
}

impl Reference<'_> {
    fn document(input: &[u8], max_depth: usize) -> Option<String> {
        let mut parser = Reference {
            input,
            at: 0,
            max_depth,
        };
        parser.space();
        let value = parser.value(0)?;
        parser.space();
        (parser.at == input.len()).then_some(value)
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.at).copied()
    }

    fn take(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.at += 1;
        Some(byte)
    }

    fn space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn word(&mut self, word: &str) -> Option<String> {
        let end = self.at + word.len();
        (self.input.get(self.at..end)? == word.as_bytes()).then(|| {
            self.at = end;
            word.to_string()
        })
    }

    fn value(&mut self, depth: usize) -> Option<String> {
        match self.peek()? {
            b'n' => self.word("null"),
            b't' => self.word("true"),
            b'f' => self.word("false"),
            b'"' => Some(quoted(&self.string()?)),
            b'[' => {
                if depth >= self.max_depth {
                    return None;
                }
                self.at += 1;
                self.space();
                let mut items = Vec::new();
                if self.peek()? == b']' {
                    self.at += 1;
                    return Some("[]".into());
                }
                loop {
                    self.space();
                    items.push(self.value(depth + 1)?);
                    self.space();
                    match self.take()? {
                        b',' => {}
                        b']' => return Some(format!("[{}]", items.join(","))),
                        _ => return None,
                    }
                }
            }
            b'{' => {
                if depth >= self.max_depth {
                    return None;
                }
                self.at += 1;
                self.space();
                let mut members = Vec::new();
                let mut keys = HashSet::new();
                if self.peek()? == b'}' {
                    self.at += 1;
                    return Some("{}".into());
                }
                loop {
                    self.space();
                    if self.peek()? != b'"' {
                        return None;
                    }
                    let key = self.string()?;
                    if !keys.insert(key.clone()) {
                        return None;
                    }
                    self.space();
                    if self.take()? != b':' {
                        return None;
                    }
                    self.space();
                    members.push(format!("{}:{}", quoted(&key), self.value(depth + 1)?));
                    self.space();
                    match self.take()? {
                        b',' => {}
                        b'}' => return Some(format!("{{{}}}", members.join(","))),
                        _ => return None,
                    }
                }
            }
            b'-' | b'0'..=b'9' => self.number(),
            _ => None,
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.at;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.at += 1;
        }
        self.at - start
    }

    fn number(&mut self) -> Option<String> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        if self.peek()? == b'0' {
            self.at += 1;
        } else if self.digits() == 0 {
            return None;
        }
        let mut float = false;
        if self.peek() == Some(b'.') {
            self.at += 1;
            float = true;
            if self.digits() == 0 {
                return None;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            float = true;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if self.digits() == 0 {
                return None;
            }
        }
        // A number directly followed by another number byte is one malformed
        // token, as in `01` or `1.2.3`.
        if matches!(
            self.peek(),
            Some(b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
        ) {
            return None;
        }
        let text = std::str::from_utf8(&self.input[start..self.at]).ok()?;
        if float {
            let value = text.parse::<f64>().ok()?;
            value.is_finite().then(|| format!("{value:?}"))
        } else {
            text.parse::<i64>().ok().map(|value| value.to_string())
        }
    }

    fn hex4(&mut self) -> Option<u32> {
        let mut code = 0;
        for _ in 0..4 {
            code = code * 16 + char::from(self.take()?).to_digit(16)?;
        }
        Some(code)
    }

    fn string(&mut self) -> Option<String> {
        self.at += 1;
        let mut bytes = Vec::new();
        loop {
            match self.take()? {
                b'"' => return String::from_utf8(bytes).ok(),
                b'\\' => {
                    let scalar = match self.take()? {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let high = self.hex4()?;
                            let code = if (0xd800..0xdc00).contains(&high) {
                                if self.take()? != b'\\' || self.take()? != b'u' {
                                    return None;
                                }
                                let low = self.hex4()?;
                                if !(0xdc00..0xe000).contains(&low) {
                                    return None;
                                }
                                0x10000 + ((high - 0xd800) << 10) + (low - 0xdc00)
                            } else {
                                high
                            };
                            char::from_u32(code)?
                        }
                        _ => return None,
                    };
                    bytes.extend_from_slice(scalar.encode_utf8(&mut [0; 4]).as_bytes());
                }
                byte if byte < 0x20 => return None,
                byte => bytes.push(byte),
            }
        }
    }
}

fn quoted(text: &str) -> String {
    let mut out = String::from('"');
    for scalar in text.chars() {
        match scalar {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            scalar if scalar < ' ' => out.push_str(&format!("\\u{:04x}", scalar as u32)),
            scalar => out.push(scalar),
        }
    }
    out.push('"');
    out
}

/// Each input must match the reference: the same canonical encoding, or a
/// located fault. Fault lines start with an upper-case variant name, which no
/// JSON text does.
fn agrees_with_reference(inputs: &[Vec<u8>]) {
    for (input, actual) in inputs.iter().zip(outcomes(inputs, "")) {
        let shown = String::from_utf8_lossy(input);
        match Reference::document(input, 64) {
            Some(expected) => assert_eq!(actual, expected, "{shown} ({})", hex(input)),
            None => assert!(
                actual.starts_with(|first: char| first.is_ascii_uppercase())
                    && !actual.starts_with("STREAM-"),
                "{shown} ({}) gave {actual}",
                hex(input)
            ),
        }
    }
}

#[test]
fn documents_decode_and_encode_canonically() {
    table(
        &[
            (b"null", "null"),
            (b" true ", "true"),
            (b"false", "false"),
            (b"0", "0"),
            (b"-0", "0"),
            (b"-0.0", "-0.0"),
            (b"1.5e3", "1500.0"),
            (b"1E-2", "0.01"),
            (b"1e2", "100.0"),
            (b"0.1", "0.1"),
            (b"5e-324", "5e-324"),
            (b"1e-400", "0.0"),
            (b"123456789012345678901234567890.0", "1.2345678901234568e29"),
            (b"9223372036854775807", "9223372036854775807"),
            (b"-9223372036854775808", "-9223372036854775808"),
            (b"\"\"", "\"\""),
            (b"\"a\\/b\"", "\"a/b\""),
            (b"[]", "[]"),
            (b"{}", "{}"),
            (b"\t\r\n [\t]\r\n", "[]"),
            (b"[ 1 , [ 2 , { \"k\" : [ ] } ] ]", "[1,[2,{\"k\":[]}]]"),
            (
                b"{\"a\":{\"b\":{\"c\":null}}}",
                "{\"a\":{\"b\":{\"c\":null}}}",
            ),
            (
                b"\"\\u00e9\\u20AC\\ud83d\\uDE00\"",
                "\"\u{e9}\u{20ac}\u{1f600}\"",
            ),
            (
                "\"\u{e9}\u{20ac}\u{1f600}\"".as_bytes(),
                "\"\u{e9}\u{20ac}\u{1f600}\"",
            ),
            (
                b"\"\\u0000\\u001f\\b\\f\\n\\r\\t\\\"\\\\\"",
                "\"\\u0000\\u001f\\b\\f\\n\\r\\t\\\"\\\\\"",
            ),
            (b"\"\\u007f\\u2028\"", "\"\u{7f}\u{2028}\""),
        ],
        "",
    );
}

#[test]
fn malformed_documents_report_the_offending_byte() {
    table(
        &[
            (b"", "UnexpectedEnd 0 1 1"),
            (b"   ", "UnexpectedEnd 3 1 4"),
            (b"[1,]", "UnexpectedByte 3 1 4"),
            (b"[,1]", "UnexpectedByte 1 1 2"),
            (b"{\"a\" 1}", "UnexpectedByte 5 1 6"),
            (b"{\"a\":1,}", "UnexpectedByte 7 1 8"),
            (b"{a:1}", "UnexpectedByte 1 1 2"),
            (b"{1:1}", "UnexpectedByte 1 1 2"),
            (b"[1 2]", "UnexpectedByte 3 1 4"),
            (b"[1}", "UnexpectedByte 2 1 3"),
            (b"{\"a\":1]", "UnexpectedByte 6 1 7"),
            (b"]", "UnexpectedByte 0 1 1"),
            (b"'a'", "UnexpectedByte 0 1 1"),
            (b"1 2", "TrailingData 2 1 3"),
            (b"{} x", "TrailingData 3 1 4"),
            (b"[]]", "TrailingData 2 1 3"),
            (b"[1] // c", "TrailingData 4 1 5"),
            (b"nul", "UnexpectedEnd 3 1 4"),
            (b"nulL", "InvalidLiteral 3 1 4"),
            (b"True", "UnexpectedByte 0 1 1"),
            (b"truefalse", "TrailingData 4 1 5"),
            (b"NaN", "UnexpectedByte 0 1 1"),
            (b"Infinity", "UnexpectedByte 0 1 1"),
            (b"-Infinity", "InvalidNumber 1 1 2"),
            (b"01", "InvalidNumber 1 1 2"),
            (b"-", "UnexpectedEnd 1 1 2"),
            (b"-x", "InvalidNumber 1 1 2"),
            (b"--1", "InvalidNumber 1 1 2"),
            (b"1.", "UnexpectedEnd 2 1 3"),
            (b"1.e3", "InvalidNumber 2 1 3"),
            (b".5", "UnexpectedByte 0 1 1"),
            (b"+1", "UnexpectedByte 0 1 1"),
            (b"1e", "UnexpectedEnd 2 1 3"),
            (b"1e+", "UnexpectedEnd 3 1 4"),
            (b"1e+-1", "InvalidNumber 3 1 4"),
            (b"1.2.3", "InvalidNumber 3 1 4"),
            (b"0x10", "TrailingData 1 1 2"),
            (b"[1a]", "UnexpectedByte 2 1 3"),
            (b"\"abc", "UnexpectedEnd 4 1 5"),
            (b"\"a\nb\"", "ControlCharacter 2 1 3"),
            (b"\"\\x\"", "InvalidEscape 2 1 3"),
            (b"\"\\u12g4\"", "InvalidEscape 5 1 6"),
            (b"\"\\ud800\"", "InvalidSurrogate 7 1 8"),
            (b"\"\\ud800\\n\"", "InvalidSurrogate 8 1 9"),
            (b"\"\\ud800\\u0041\"", "InvalidSurrogate 12 1 13"),
            (b"\"\\udc00\"", "InvalidSurrogate 6 1 7"),
            (b"\xef\xbb\xbf[]", "UnexpectedByte 0 1 1"),
            (b"\xc3\xa9", "UnexpectedByte 0 1 1"),
            (b"\"\xff\"", "InvalidUtf8 1 1 2"),
            (b"\"\x80\"", "InvalidUtf8 1 1 2"),
            (b"\"\xc3\"", "InvalidUtf8 2 1 3"),
            (b"\"\xc0\xaf\"", "InvalidUtf8 1 1 2"),
            (b"\"\xe0\x80\x80\"", "InvalidUtf8 2 1 3"),
            (b"\"\xed\xa0\x80\"", "InvalidUtf8 2 1 3"),
            (b"\"\xf4\x90\x80\x80\"", "InvalidUtf8 2 1 3"),
            (b"\"\xf5\x80\x80\x80\"", "InvalidUtf8 1 1 2"),
            // A column is one scalar: the two bytes of `é` advance it once.
            (
                "[\n  1,\n  \"\u{e9}\" x\n]".as_bytes(),
                "UnexpectedByte 14 3 7",
            ),
            // A line ends at `\n`; the `\r` before it is an ordinary column.
            (b"[\r\n1,\r\n?]", "UnexpectedByte 7 3 1"),
        ],
        "",
    );
}

#[test]
fn long_string_runs_keep_content_and_positions() {
    // Runs longer than the native scan step, ended by each kind of byte.
    table(
        &[
            (
                b"\"abcdefghijklmnop\\tqrstuvwxyz 0123456789\"",
                "\"abcdefghijklmnop\\tqrstuvwxyz 0123456789\"",
            ),
            (
                "[\"plain run then \u{e9} and more plain text\", \"~\u{7f} {}[]:,\"]".as_bytes(),
                "[\"plain run then \u{e9} and more plain text\",\"~\u{7f} {}[]:,\"]",
            ),
            (
                b"{\"a long plain key\":\"with \\\"quotes\\\" and \\\\ inside it\"}",
                "{\"a long plain key\":\"with \\\"quotes\\\" and \\\\ inside it\"}",
            ),
            (b"\"abcdefghijklmnop\x01\"", "ControlCharacter 17 1 18"),
            (b"\"abcdefghijklmnop\nq\"", "ControlCharacter 17 1 18"),
            (b"\"abcdefghij\xff\"", "InvalidUtf8 11 1 12"),
            (b"\n\"abcdefghijklmnop\\q\"", "InvalidEscape 19 2 19"),
            (b"\"abcdefghijkl", "UnexpectedEnd 13 1 14"),
            (b"[\"abcdefghijklmnop\" 1]", "UnexpectedByte 20 1 21"),
        ],
        "",
    );
}

#[test]
fn every_truncation_is_an_unexpected_end_at_its_length() {
    let document =
        "{\"key\":[1,-2.5e+3,true,false,null,\"s\\u00e9\\ud83d\\ude00\\n\u{e9}\",{\"n\":{}}],\"\u{20ac}\":[]}";
    native(
        &format!(
            r#"
bounds = std.json.limits()
source = unhex("{digits}")?
mut wrong = 0
for stop in range(source.len()):
    prefix = source.slice(0, stop)?
    match std.json.parse(&prefix, &bounds):
        case Ok(_):
            wrong += 1
        case Err(error):
            match &error.fault:
                case JsonFault.UnexpectedEnd:
                    if error.offset != stop:
                        wrong += 1
                case _:
                    wrong += 1
print(source.len(), wrong, described(std.json.parse(&source, &bounds))?)?
"#,
            digits = hex(document.as_bytes())
        ),
        "79 0 {\"key\":[1,-2500.0,true,false,null,\"s\u{e9}\u{1f600}\\n\u{e9}\",{\"n\":{}}],\"\u{20ac}\":[]}",
    );
}

#[test]
fn duplicate_keys_are_rejected_unless_the_limits_keep_them() {
    table(
        &[
            (b"{\"a\":1,\"b\":2,\"a\":3}", "DuplicateKey 13 1 14"),
            (b"{\"a\":1,\"\\u0061\":2}", "DuplicateKey 7 1 8"),
            (b"{\"\":1,\"\":2}", "DuplicateKey 6 1 7"),
            (
                b"{\"a\":{\"a\":1},\"b\":[{\"a\":1},{\"a\":2}]}",
                "{\"a\":{\"a\":1},\"b\":[{\"a\":1},{\"a\":2}]}",
            ),
            (b"{\"a\":{\"b\":1,\"b\":2}}", "DuplicateKey 12 1 13"),
        ],
        "",
    );
    table(
        &[
            (b"{\"a\":1,\"b\":2,\"a\":3}", "{\"a\":1,\"b\":2,\"a\":3}"),
            (b"{\"a\":{\"b\":1,\"b\":2}}", "{\"a\":{\"b\":1,\"b\":2}}"),
        ],
        "bounds.duplicate_keys = DuplicateKeys.Keep",
    );
    native(
        r#"
mut bounds = std.json.limits()
bounds.duplicate_keys = DuplicateKeys.Keep
value = std.json.parse_text("{\"a\":1,\"b\":2,\"a\":3}", &bounds)?
match &value:
    case Json.Object(members):
        print(len(members), std.json.find(members, "a"), std.json.find(members, "b"), std.json.find(members, "c"))?
    case _:
        print("not an object")?
strict = std.json.limits()
source = std.bytes.from_text("{\"a\":1,\"a\":2}")?
print(events(&source, &strict, 0)?)?
"#,
        r#"
3 Option[i64].Some(0) Option[i64].Some(1) Option[i64].Nothing
StartObject Key("a") Number("1") Key("a") Number("2") EndObject
"#,
    );
}

#[test]
fn integers_are_exact_and_floats_are_finite() {
    table(
        &[
            (b"9223372036854775808", "NumberOutOfRange 0 1 1"),
            (b"[-9223372036854775809]", "NumberOutOfRange 1 1 2"),
            (b"[1, 18446744073709551615]", "NumberOutOfRange 4 1 5"),
            (
                b"{\"n\":1000000000000000000000000000000000000000}",
                "NumberOutOfRange 5 1 6",
            ),
            (b"[1e309]", "NumberOutOfRange 1 1 2"),
            (b"[-1E+309]", "NumberOutOfRange 1 1 2"),
            (b"1.7976931348623157e308", "1.7976931348623157e308"),
            (b"9223372036854775808.0", "9.223372036854776e18"),
            (b"[10, 10.0, 1e1]", "[10,10.0,10.0]"),
        ],
        "",
    );
    native(
        r#"
bounds = std.json.limits()
source = std.bytes.from_text("[18446744073709551615, 1.0, 1]")?
mut decoder = std.json.decoder(&bounds)?
decoder.feed(&source, 0, source.len())?
decoder.finish()
drop(decoder.next()?)
match decoder.next()?:
    case Event.Number(text):
        print(u64.parse(text)?, str.repr(i64.parse(text))?)?
    case _:
        print("other")?
value = std.json.parse_text("[1, 1.0]", &bounds)?
match &value:
    case Json.Array(items):
        for item in items:
            match item:
                case Json.Int(number):
                    print("Int", *number)?
                case Json.Float(number):
                    print("Float", *number)?
                case _:
                    print("other")?
    case _:
        print("other")?
"#,
        r#"
18446744073709551615 Result[i64, ParseError].Err(ParseError.OutOfRange)
Int 1
Float 1.0
"#,
    );
}

#[test]
fn nesting_is_bounded_by_the_depth_limit() {
    let nested = |depth: usize| [b"[".repeat(depth), b"]".repeat(depth)].concat();
    table(
        &[
            (&nested(3), "[[[]]]"),
            (&nested(4), "DepthExceeded 3 1 4"),
            (b"{\"a\":{\"a\":{\"a\":1}}}", "{\"a\":{\"a\":{\"a\":1}}}"),
            (
                b"{\"a\":{\"a\":{\"a\":{\"a\":1}}}}",
                "DepthExceeded 15 1 16",
            ),
            (b"[[1],[2],[[3]]]", "[[1],[2],[[3]]]"),
        ],
        "bounds.max_depth = 3",
    );
    table(
        &[
            (b"1", "1"),
            (b"\"x\"", "\"x\""),
            (b"[]", "DepthExceeded 0 1 1"),
            (b" {}", "DepthExceeded 1 1 2"),
        ],
        "bounds.max_depth = 0",
    );
    table(
        &[
            (&nested(64), &String::from_utf8(nested(64)).unwrap()),
            (&nested(65), "DepthExceeded 64 1 65"),
            (&b"[".repeat(100_000), "DepthExceeded 64 1 65"),
        ],
        "",
    );
    table(
        &[
            (&nested(1024), &String::from_utf8(nested(1024)).unwrap()),
            (&nested(1025), "DepthExceeded 1024 1 1025"),
        ],
        "bounds.max_depth = 1024",
    );
}

#[test]
fn invalid_limits_are_rejected_before_any_work() {
    native(
        r#"
value = Json.Null
for which in range(4):
    mut bounds = std.json.limits()
    if which == 0:
        bounds.max_depth = 1025
    elif which == 1:
        bounds.max_depth = -1
    elif which == 2:
        bounds.max_input = -1
    else:
        bounds.max_output = -1
    print(described(std.json.parse_text("1", &bounds))?)?
    match std.json.decoder(&bounds):
        case Ok(_):
            print("decoder")?
        case Err(error):
            print(placed(&error)?)?
    match std.json.encoder(&bounds):
        case Ok(_):
            print("encoder")?
        case Err(error):
            print(placed(&error)?)?
    match std.json.encode(&value, &bounds):
        case Ok(_):
            print("encode")?
        case Err(error):
            print(placed(&error)?)?
"#,
        &"InvalidLimits 0 0 0\n".repeat(16),
    );
}

#[test]
fn input_and_output_sizes_are_bounded() {
    table(
        &[
            (b"[1]", "[1]"),
            (b"[10]", "[10]"),
            (b"[1, 2]", "InputTooLarge 4 0 0"),
            (b"[100]", "InputTooLarge 4 0 0"),
        ],
        "bounds.max_input = 4",
    );
    table(
        &[(b"", "UnexpectedEnd 0 1 1"), (b"1", "InputTooLarge 0 0 0")],
        "bounds.max_input = 0",
    );
    native(
        r#"
mut bounds = std.json.limits()
value = std.json.parse_text("[1,2,\"three\"]", &bounds)?
bounds.max_output = 13
print(std.json.encode_text(&value, &bounds)?)?
bounds.max_output = 12
match std.json.encode(&value, &bounds):
    case Ok(_):
        print("encoded")?
    case Err(error):
        print(placed(&error)?)?
mut sink = std.io.cursor(std.bytes.empty()?)
print(settled(std.json.write_value(&mut sink, &value, &bounds))?, sink.data.len())?
# The limit counts every byte produced, including bytes already taken.
bounds.max_output = 4
mut encoder = std.json.encoder(&bounds)?
encoder.start_array()?
encoder.integer(12)?
print(encoder.take()?.to_text()?, encoder.pending(), encoder.written())?
print(settled(encoder.integer(3))?)?
print(settled(encoder.end_array())?)?
# A feed that would pass the limit is refused whole.
bounds.max_input = 3
source = std.bytes.from_text("[1, 2]")?
mut decoder = std.json.decoder(&bounds)?
print(settled(decoder.feed(&source, 0, 3))?)?
print(settled(decoder.feed(&source, 3, 4))?)?
"#,
        r#"
[1,2,"three"]
OutputTooLarge 12 0 0
OutputTooLarge 12 0 0 0
[12 0 3
OutputTooLarge 4 0 0
InvalidState 4 0 0
ok
InputTooLarge 3 0 0
"#,
    );
}

#[test]
fn events_arrive_in_document_order() {
    native(
        r#"
bounds = std.json.limits()
source = std.bytes.from_text("{\"a\":[1,true,null,\"x\",-2.50e1],\"b\":{},\"c\":false}")?
print(events(&source, &bounds, 0)?)?
bare = std.bytes.from_text(" 12 ")?
print(events(&bare, &bounds, 0)?)?
# The two events that carry no token write nothing.
mut encoder = std.json.encoder(&bounds)?
encoder.event(&Event.Pending)?
encoder.event(&Event.End)?
encoder.event(&Event.Bool(True))?
encoder.event(&Event.End)?
print(encoder.take()?.to_text()?, encoder.is_complete())?
"#,
        r#"
StartObject Key("a") StartArray Number("1") Bool(True) Null Text("x") Number("-2.50e1") EndArray Key("b") StartObject EndObject Key("c") Bool(False) EndObject
Number("12")
true True
"#,
    );
}

#[test]
fn input_may_be_split_between_any_two_bytes() {
    let document =
        " {\"k\\u00e9\\ud83d\\ude00\\\\\":[-1.25e+10,0,true,false,null,\"\u{e9}\u{20ac}\u{1f600}\\n\",{}],\"\":[[]]} ";
    native(
        &format!(
            r#"
bounds = std.json.limits()
source = unhex("{digits}")?
whole = events(&source, &bounds, source.len())?
mut wrong = 0
for split in range(source.len() + 1):
    if events(&source, &bounds, split)? != whole:
        wrong += 1
# One byte per feed.
mut decoder = std.json.decoder(&bounds)?
mut names = list[str]()?
for index in range(source.len()):
    decoder.feed(&source, index, index + 1)?
    drain(&mut decoder, &mut names)?
decoder.finish()
drain(&mut decoder, &mut names)?
print(wrong, " ".join(&names)? == whole)?
print(whole)?
"#,
            digits = hex(document.as_bytes())
        ),
        "0 True\nStartObject Key(\"k\u{e9}\u{1f600}\\\\\") StartArray Number(\"-1.25e+10\") Number(\"0\") Bool(True) Bool(False) Null Text(\"\u{e9}\u{20ac}\u{1f600}\\n\") StartObject EndObject EndArray Key(\"\") StartArray StartArray EndArray EndArray EndObject",
    );
}

#[test]
fn the_decoder_reports_pending_end_and_misuse() {
    native(
        r#"
bounds = std.json.limits()
source = std.bytes.from_text("[12, 3]x")?
mut decoder = std.json.decoder(&bounds)?
print(str.repr(&decoder.next()?)?, decoder.offset(), decoder.depth())?
print(settled(decoder.feed(&source, 3, 2))?)?
print(settled(decoder.feed(&source, 0, 9))?)?
decoder.feed(&source, 0, 3)?
print(str.repr(&decoder.next()?)?, decoder.offset(), decoder.depth())?
# The number may continue, so nothing is reported until more input arrives.
print(str.repr(&decoder.next()?)?, decoder.offset())?
decoder.feed(&source, 3, 6)?
print(str.repr(&decoder.next()?)?, decoder.offset())?
print(placed(&decoder.reject(JsonFault.NumberOutOfRange))?)?
print(str.repr(&decoder.next()?)?, decoder.offset())?
decoder.feed(&source, 6, 7)?
print(str.repr(&decoder.next()?)?, decoder.depth())?
print(str.repr(&decoder.next()?)?, decoder.depth())?
print(str.repr(&decoder.next()?)?)?
decoder.finish()
print(settled(decoder.feed(&source, 7, 8))?)?
print(str.repr(&decoder.next()?)?)?
print(str.repr(&decoder.next()?)?)?

mut broken = std.json.decoder(&bounds)?
broken.feed(&source, 0, 8)?
broken.finish()
mut names = list[str]()?
drain(&mut broken, &mut names)?
print(" ".join(&names)?)?
match broken.next():
    case Ok(_):
        print("event")?
    case Err(error):
        print(placed(&error)?)?
print(settled(broken.feed(&source, 0, 1))?)?
"#,
        r#"
Pending 0 0
OutOfRange 0 1 1
OutOfRange 0 1 1
StartArray 1 1
Pending 3
Number("12") 3
NumberOutOfRange 1 1 2
Pending 6
Number("3") 1
EndArray 0
Pending
InvalidState 7 1 8
End
End
StartArray Number("12") Number("3") EndArray TrailingData 7 1 8
InvalidState 7 1 8
InvalidState 7 1 8
"#,
    );
}

#[test]
fn short_and_buffered_streams_decode_like_whole_buffers() {
    // Larger than the decoder's read window, so the window is refilled.
    let mut document = String::from("[");
    for index in 0..3000 {
        document.push_str(&format!("{{\"n\":{index},\"s\":\"v{index}\\n\"}},"));
    }
    document.push_str("null]");
    native(
        &format!(
            r#"
bounds = std.json.limits()
source = unhex("{digits}")?
whole = described(std.json.parse(&source, &bounds))?
mut cursor = std.io.cursor(source.copy()?)
print(described(std.json.read_value(&mut cursor, &bounds))? == whole)?
print(trickled(&source, &bounds, 7)? == whole, trickled(&source, &bounds, 5000)? == whole)?
mut buffered = std.io.buffered_reader(std.io.cursor(source.copy()?), 16)?
print(described(std.json.read_value(&mut buffered, &bounds))? == whole)?
print(len(whole), whole.endswith("{{\"n\":2999,\"s\":\"v2999\\n\"}},null]"))?

# Events straight from a reader into an encoder and on to a writer, without
# a tree: memory stays bounded by the window and the current token.
mut reader = Trickle(source.copy()?, 0, 11)
mut decoder = std.json.decoder(&bounds)?
mut encoder = std.json.encoder(&bounds)?
mut sink = std.io.cursor(std.bytes.empty()?)
mut most = 0
while True:
    event = std.json.read_event(&mut decoder, &mut reader)?
    match &event:
        case Event.End:
            break
        case _:
            pass
    encoder.event(&event)?
    if encoder.pending() > most:
        most = encoder.pending()
    std.json.write_pending(&mut encoder, &mut sink)?
print(encoder.is_complete(), sink.data.to_text()? == whole, most < 16, encoder.written() == len(whole))?
print(str.repr(&std.json.read_event(&mut decoder, &mut reader)?)?)?
"#,
            digits = hex(document.as_bytes())
        ),
        "True\nTrue True\nTrue\n72786 True\nTrue True True True\nEnd",
    );
}

#[test]
fn stream_failures_are_reported_with_the_bytes_examined() {
    let source = format!(
        "{HEADER}{}",
        r#"
# Interrupts every other call, and fails or misreports after `good` bytes.
pub class Flaky:
    pub data: Bytes
    pub at: i64
    pub good: i64
    pub lie: bool
    pub calls: i64

    pub def read(self: &mut Flaky, buffer: &mut Bytes, start: i64, stop: i64) -> Result[i64, StreamError]:
        self.calls += 1
        if self.calls % 2 == 1:
            return Err(StreamError.Interrupted)
        if self.at >= self.good:
            if self.lie:
                return Ok(stop - start + 1)
            return Err(StreamError.Io(IoError.Closed))
        if self.at >= self.data.len():
            return Ok(0)
        match buffer.overwrite(start, &self.data, self.at, self.at + 1):
            case Ok(_):
                pass
            case Err(_):
                return Err(StreamError.OutOfRange)
        self.at += 1
        Ok(1)

# Accepts one byte per call and fails once `room` bytes have arrived.
pub class Sip:
    pub data: Bytes
    pub room: i64

    pub def write[S: ByteSource](self: &mut Sip, source: &S, start: i64, stop: i64) -> Result[i64, StreamError]:
        if start == stop:
            return Ok(0)
        if self.data.len() >= self.room:
            return Err(StreamError.Io(IoError.NotWritable))
        match self.data.extend_range(source, start, start + 1):
            case Ok(_):
                Ok(1)
            case Err(_):
                Err(StreamError.OutOfRange)

    pub def flush(self: &mut Sip) -> Result[(), StreamError]:
        Ok(())

def main() -> Result[(), AllocError | IoError | std.encoding.EncodingError | std.bytes.BytesError | std.json.JsonError]:
    bounds = std.json.limits()
    text = "[1,\n \"two\", 3]"
    mut steady = Flaky(std.bytes.from_text(text)?, 0, 100, False, 0)
    print(described(std.json.read_value(&mut steady, &bounds))?, steady.calls)?
    mut closed = Flaky(std.bytes.from_text(text)?, 0, 7, False, 0)
    print(described(std.json.read_value(&mut closed, &bounds))?)?
    mut liar = Flaky(std.bytes.from_text(text)?, 0, 2, True, 0)
    print(described(std.json.read_value(&mut liar, &bounds))?)?

    value = std.json.parse_text(text, &bounds)?
    mut sip = Sip(std.bytes.empty()?, 100)
    print(settled(std.json.write_value(&mut sip, &value, &bounds))?, sip.data.to_text()?)?
    mut full = Sip(std.bytes.empty()?, 4)
    mut encoder = std.json.encoder(&bounds)?
    encoder.value(&value)?
    print(settled(std.json.write_pending(&mut encoder, &mut full))?, full.data.to_text()?, encoder.pending())?
    # A retry delivers only what the first writer refused.
    mut rest = Sip(std.bytes.empty()?, 100)
    print(settled(std.json.write_pending(&mut encoder, &mut rest))?, rest.data.to_text()?, encoder.pending())?
    Ok(())
"#
    );
    assert_eq!(
        output(&source),
        "[1,\"two\",3] 30\n\
         Stream(Io(IoError.Closed)) 7 2 4\n\
         Stream(InvalidProgress) 2 1 3\n\
         ok [1,\"two\",3]\n\
         Stream(Io(IoError.NotWritable)) 11 0 0 [1,\" 7\n\
         ok two\",3] 0"
    );
}

#[test]
fn the_encoder_writes_compact_text_with_minimal_escapes() {
    native(
        r#"
bounds = std.json.limits()
mut encoder = std.json.encoder(&bounds)?
print(encoder.is_complete(), encoder.pending())?
encoder.start_object()?
encoder.key("id")?
encoder.unsigned(18446744073709551615u64)?
encoder.key("xs")?
encoder.start_array()?
encoder.integer(-5)?
encoder.integer(i64.MIN)?
encoder.decimal(0.5)?
encoder.decimal(1e21)?
encoder.decimal(-0.0)?
encoder.decimal(100.0)?
encoder.number("1.50E+3")?
encoder.boolean(True)?
encoder.boolean(False)?
encoder.null()?
encoder.start_array()?
encoder.end_array()?
encoder.start_object()?
encoder.end_object()?
encoder.end_array()?
encoder.key("")?
controls = "".join(&[chr(0)?, chr(1)?, chr(8)?, chr(9)?, chr(10)?, chr(12)?, chr(13)?, chr(27)?, chr(31)?, " ", chr(127)?]?)?
encoder.text(&controls)?
encoder.key("quote\"slash\\/")?
encoder.text("é€😀")?
print(encoder.is_complete())?
encoder.end_object()?
print(encoder.is_complete(), encoder.pending() == encoder.written())?
encoded = encoder.take()?
print(encoded.to_text()?)?
print(encoder.pending(), encoder.written() == encoded.len())?
print(described(std.json.parse(&encoded, &bounds))? == encoded.to_text()?)?
"#,
        "False 0\nFalse\nTrue True\n{\"id\":18446744073709551615,\"xs\":[-5,-9223372036854775808,0.5,1e21,-0.0,100.0,1.50E+3,true,false,null,[],{}],\"\":\"\\u0000\\u0001\\b\\t\\n\\f\\r\\u001b\\u001f \u{7f}\",\"quote\\\"slash\\\\/\":\"\u{e9}\u{20ac}\u{1f600}\"}\n0 True\nFalse",
    );
}

#[test]
fn the_encoder_rejects_calls_that_do_not_form_a_document() {
    native(
        r#"
mut bounds = std.json.limits()
bounds.max_depth = 2
for which in range(21):
    mut encoder = std.json.encoder(&bounds)?
    mut result: Result[(), JsonError] = Ok(())
    if which == 0:
        encoder.null()?
        result = encoder.null()
    elif which == 1:
        result = encoder.key("a")
    elif which == 2:
        result = encoder.end_array()
    elif which == 3:
        result = encoder.end_object()
    elif which == 4:
        encoder.start_object()?
        result = encoder.integer(1)
    elif which == 5:
        encoder.start_object()?
        encoder.key("a")?
        result = encoder.key("b")
    elif which == 6:
        encoder.start_object()?
        encoder.key("a")?
        result = encoder.end_object()
    elif which == 7:
        encoder.start_object()?
        result = encoder.end_array()
    elif which == 8:
        encoder.start_array()?
        result = encoder.end_object()
    elif which == 9:
        encoder.start_array()?
        result = encoder.key("a")
    elif which == 10:
        encoder.start_array()?
        encoder.end_array()?
        result = encoder.start_array()
    elif which == 11:
        encoder.start_array()?
        encoder.start_array()?
        result = encoder.start_object()
    elif which == 12:
        result = encoder.decimal(f64.NAN)
    elif which == 13:
        result = encoder.decimal(f64.INF)
    elif which == 14:
        encoder.start_array()?
        result = encoder.decimal(-f64.INF)
    elif which == 15:
        result = encoder.number("01")
    elif which == 16:
        result = encoder.number("")
    elif which == 17:
        result = encoder.number("1e")
    elif which == 18:
        result = encoder.number(" 1")
    elif which == 19:
        result = encoder.number("NaN")
    else:
        result = encoder.number("1 ")
    print(which, settled(result)?, settled(encoder.null())?, encoder.is_complete())?
"#,
        r#"
0 InvalidState 4 0 0 InvalidState 4 0 0 False
1 InvalidState 0 0 0 InvalidState 0 0 0 False
2 InvalidState 0 0 0 InvalidState 0 0 0 False
3 InvalidState 0 0 0 InvalidState 0 0 0 False
4 InvalidState 1 0 0 InvalidState 1 0 0 False
5 InvalidState 5 0 0 InvalidState 5 0 0 False
6 InvalidState 5 0 0 InvalidState 5 0 0 False
7 InvalidState 1 0 0 InvalidState 1 0 0 False
8 InvalidState 1 0 0 InvalidState 1 0 0 False
9 InvalidState 1 0 0 InvalidState 1 0 0 False
10 InvalidState 2 0 0 InvalidState 2 0 0 False
11 DepthExceeded 2 0 0 InvalidState 2 0 0 False
12 NonFiniteNumber 0 0 0 InvalidState 0 0 0 False
13 NonFiniteNumber 0 0 0 InvalidState 0 0 0 False
14 NonFiniteNumber 1 0 0 InvalidState 1 0 0 False
15 InvalidNumber 0 0 0 InvalidState 0 0 0 False
16 InvalidNumber 0 0 0 InvalidState 0 0 0 False
17 InvalidNumber 0 0 0 InvalidState 0 0 0 False
18 InvalidNumber 0 0 0 InvalidState 0 0 0 False
19 InvalidNumber 0 0 0 InvalidState 0 0 0 False
20 InvalidNumber 0 0 0 InvalidState 0 0 0 False
"#,
    );
}

#[test]
fn trees_built_by_hand_encode_and_reject_what_json_cannot_hold() {
    native(
        r#"
bounds = std.json.limits()
mut items = list[Json]()?
items.append(Json.Int(1))?
items.append(Json.Float(2.5))?
items.append(Json.Text("three"))?
mut members = list[Member]()?
members.append(Member("items", Json.Array(items)))?
members.append(Member("none", Json.Null))?
members.append(Member("yes", Json.Bool(True)))?
value = Json.Object(members)
print(std.json.encode_text(&value, &bounds)?)?
mut sink = std.io.cursor(std.bytes.empty()?)
std.json.write_value(&mut sink, &value, &bounds)?
print(sink.data.to_text()?)?

mut bad = list[Json]()?
bad.append(Json.Int(1))?
bad.append(Json.Float(f64.NAN))?
match std.json.encode(&Json.Array(bad), &bounds):
    case Ok(_):
        print("encoded")?
    case Err(error):
        print(placed(&error)?)?

# The encoder does not check member names: a tree may repeat a key.
mut twice = list[Member]()?
twice.append(Member("a", Json.Int(1)))?
twice.append(Member("a", Json.Int(2)))?
text = std.json.encode_text(&Json.Object(twice), &bounds)?
print(text, described(std.json.parse_text(&text, &bounds))?)?

mut shallow = std.json.limits()
shallow.max_depth = 1
mut inner = list[Json]()?
inner.append(Json.Array(list[Json]()?))?
match std.json.encode(&Json.Array(inner), &shallow):
    case Ok(_):
        print("encoded")?
    case Err(error):
        print(placed(&error)?)?
"#,
        r#"
{"items":[1,2.5,"three"],"none":null,"yes":true}
{"items":[1,2.5,"three"],"none":null,"yes":true}
NonFiniteNumber 2 0 0
{"a":1,"a":2} DuplicateKey 7 1 8
DepthExceeded 1 0 0
"#,
    );
}

#[test]
fn typed_codecs_are_ordinary_functions_over_events() {
    let source = format!(
        "{HEADER}{}",
        r#"
class Point:
    x: i64
    y: i64

def coordinate(decoder: &mut std.json.Decoder) -> Result[i64, JsonError]:
    match decoder.next()?:
        case Event.Number(text):
            match i64.parse(text):
                case Ok(value):
                    Ok(value)
                case Err(_):
                    Err(decoder.reject(JsonFault.NumberOutOfRange))
        case _:
            Err(decoder.reject(JsonFault.UnexpectedByte))

# Reads exactly `[x, y]`.
def decode_point(decoder: &mut std.json.Decoder) -> Result[Point, JsonError]:
    match decoder.next()?:
        case Event.StartArray:
            pass
        case _:
            return Err(decoder.reject(JsonFault.UnexpectedByte))
    x = coordinate(decoder)?
    y = coordinate(decoder)?
    match decoder.next()?:
        case Event.EndArray:
            Ok(Point(x, y))
        case _:
            Err(decoder.reject(JsonFault.UnexpectedByte))

def encode_point(encoder: &mut std.json.Encoder, point: &Point) -> Result[(), JsonError]:
    encoder.start_array()?
    encoder.integer(point.x)?
    encoder.integer(point.y)?
    encoder.end_array()

def attempt(text: &str) -> Result[(), AllocError | IoError | std.bytes.BytesError | std.json.JsonError]:
    bounds = std.json.limits()
    source = std.bytes.from_text(text)?
    mut decoder = std.json.decoder(&bounds)?
    decoder.feed(&source, 0, source.len())?
    decoder.finish()
    match decode_point(&mut decoder):
        case Ok(point):
            mut encoder = std.json.encoder(&bounds)?
            encode_point(&mut encoder, &point)?
            print(point.x + point.y, encoder.take()?.to_text()?)?
        case Err(error):
            print(placed(&error)?)?
    Ok(())

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | std.json.JsonError]:
    attempt(" [3, -4]")?
    attempt("[3, 4.5]")?
    attempt("[3, \"4\"]")?
    attempt("[3, 4, 5]")?
    attempt("[3, 4")?
    Ok(())
"#
    );
    assert_eq!(
        output(&source),
        "-1 [3,-4]\n\
         NumberOutOfRange 4 1 5\n\
         UnexpectedByte 4 1 5\n\
         UnexpectedByte 7 1 8\n\
         UnexpectedEnd 5 1 6"
    );
}

#[test]
fn codecs_run_in_worker_threads() {
    let source = format!(
        "{HEADER}{}",
        r#"
def work(data: Bytes) -> ():
    bounds = std.json.limits()
    value = std.json.parse(&data, &bounds).unwrap()
    print(std.json.encode_text(&value, &bounds).unwrap()).unwrap()

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | ThreadError]:
    data = std.bytes.from_text(" { \"a\" : [ 1 , 2 ] } ")?
    with spawn(work, data)?:
        pass
    Ok(())
"#
    );
    assert_eq!(output(&source), "{\"a\":[1,2]}");
}

/// Injects a failure at every allocation of each operation in turn. Each must
/// report `Allocation` until enough allocations succeed, and process exit
/// checks that every partial tree and buffer was released.
#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_is_reported_at_every_step_and_leaks_nothing() {
    native(
        r#"
bounds = std.json.limits()
text = "{\"a key too long for inline storage\":[1,2.5,\"a value too long for inline storage\",{\"k\":null,\"again\":[true,false]}],\"z\":\"\\u00e9\"}"
source = std.bytes.from_text(text)?
value = std.json.parse(&source, &bounds)?
mut markers = list[str]()?
for count in range(400):
    markers.append("".join(&["__test_fail_allocations_after_", str.from(count)?, "__"]?)?)?
for which in range(3):
    mut refused = 0
    mut other = 0
    mut first = -1
    for count in range(400):
        mut fault = 0
        if which == 0:
            print(markers[count])?
            result = std.json.parse(&source, &bounds)
            print("__test_restore_allocations__")?
            match result:
                case Ok(_):
                    pass
                case Err(error):
                    match &error.fault:
                        case JsonFault.Allocation(_):
                            fault = 1
                        case _:
                            fault = 2
        elif which == 1:
            mut reader = Trickle(source.copy()?, 0, 5)
            print(markers[count])?
            result = std.json.read_value(&mut reader, &bounds)
            print("__test_restore_allocations__")?
            match result:
                case Ok(_):
                    pass
                case Err(error):
                    match &error.fault:
                        case JsonFault.Allocation(_):
                            fault = 1
                        case _:
                            fault = 2
        else:
            print(markers[count])?
            result = std.json.encode(&value, &bounds)
            print("__test_restore_allocations__")?
            match result:
                case Ok(_):
                    pass
                case Err(error):
                    match &error.fault:
                        case JsonFault.Allocation(_):
                            fault = 1
                        case _:
                            fault = 2
        if fault == 1:
            refused += 1
        elif fault == 2:
            other += 1
        elif first < 0:
            first = count
    # Every count below the first success failed, and none above it did.
    print(which, refused > 5, refused == first, other)?
"#,
        r#"
0 True True 0
1 True True 0
2 True True 0
"#,
    );
}

/// xorshift64: the fuzz inputs are the same on every run.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }

    fn pick<'a>(&mut self, choices: &[&'a str]) -> &'a str {
        choices[self.below(choices.len())]
    }
}

fn random_space(random: &mut Random, out: &mut String) {
    for _ in 0..random.below(3) {
        out.push_str(random.pick(&[" ", "\n", "\t", "\r"]));
    }
}

fn random_string(random: &mut Random, out: &mut String) {
    out.push('"');
    for _ in 0..random.below(5) {
        out.push_str(random.pick(&[
            "a",
            "k",
            "0",
            " ",
            "\u{e9}",
            "\u{20ac}",
            "\u{1f600}",
            "\\n",
            "\\\"",
            "\\\\",
            "\\/",
            "\\u0041",
            "\\u00e9",
            "\\ud83d\\ude00",
            "\\uD834\\uDD1E",
            "\\u0000",
            "\\t",
        ]));
    }
    out.push('"');
}

fn random_value(random: &mut Random, depth: usize, out: &mut String) {
    random_space(random, out);
    let kinds = if depth >= 4 { 6 } else { 8 };
    match random.below(kinds) {
        0 => out.push_str(random.pick(&["null", "true", "false"])),
        1 | 2 => out.push_str(random.pick(&[
            "0",
            "-0",
            "7",
            "-12",
            "9223372036854775807",
            "-9223372036854775808",
            "0.5",
            "-0.0",
            "1e2",
            "1E-3",
            "2.50e+10",
            "123456789.125",
            "1e308",
            "5e-324",
            "0.1",
        ])),
        3..=5 => random_string(random, out),
        6 => {
            out.push('[');
            for index in 0..random.below(4) {
                if index > 0 {
                    out.push(',');
                }
                random_value(random, depth + 1, out);
            }
            random_space(random, out);
            out.push(']');
        }
        _ => {
            out.push('{');
            for index in 0..random.below(4) {
                if index > 0 {
                    out.push(',');
                }
                random_space(random, out);
                // Distinct keys, so the document stays valid.
                let mut key = String::new();
                random_string(random, &mut key);
                key.insert_str(1, &index.to_string());
                out.push_str(&key);
                random_space(random, out);
                out.push(':');
                random_value(random, depth + 1, out);
            }
            random_space(random, out);
            out.push('}');
        }
    }
    random_space(random, out);
}

/// Bytes that steer the decoder into its less common states.
const INTERESTING: &[u8] = b"{}[]\",:\\ \n0123456789-+.eEtrufalsn/b\x00\x1f\x7f\x80\xbf\xc2\xc3\xa9\xe0\xa0\xed\xf0\x90\xf4\xff";

fn mutate(random: &mut Random, seed: &[u8]) -> Vec<u8> {
    let mut bytes = seed.to_vec();
    for _ in 0..1 + random.below(3) {
        let at = random.below(bytes.len() + 1);
        let byte = INTERESTING[random.below(INTERESTING.len())];
        match random.below(5) {
            0 if at < bytes.len() => {
                bytes.remove(at);
            }
            1 => bytes.insert(at, byte),
            2 if at < bytes.len() => bytes[at] = byte,
            3 => bytes.truncate(at),
            _ if at < bytes.len() => {
                let end = (at + 1 + random.below(6)).min(bytes.len());
                let slice = bytes[at..end].to_vec();
                bytes.splice(at..at, slice);
            }
            _ => bytes.push(byte),
        }
    }
    bytes
}

#[test]
fn generated_documents_match_the_reference_parser() {
    let mut random = Random(0x9e37_79b9_7f4a_7c15);
    let mut inputs = Vec::new();
    for _ in 0..150 {
        let mut text = String::new();
        random_value(&mut random, 0, &mut text);
        assert!(
            Reference::document(text.as_bytes(), 64).is_some(),
            "generator produced an invalid document: {text}"
        );
        inputs.push(text.into_bytes());
    }
    agrees_with_reference(&inputs);
}

#[test]
fn mutated_documents_match_the_reference_parser() {
    let mut random = Random(0x2545_f491_4f6c_dd1d);
    let mut seeds = Vec::new();
    for _ in 0..40 {
        let mut text = String::new();
        random_value(&mut random, 0, &mut text);
        seeds.push(text.into_bytes());
    }
    let mut inputs = Vec::new();
    for index in 0..600 {
        inputs.push(mutate(&mut random, &seeds[index % seeds.len()]));
    }
    agrees_with_reference(&inputs);
}

/// `tests/fixtures/std_json/corpus.txt` keeps hand-picked documents and every
/// input that once exposed a defect. Each line is `accept` or `reject`, then
/// the input in hex; the verdict is also checked against the reference parser.
#[test]
fn the_regression_corpus_keeps_its_verdicts() {
    let corpus = include_str!("fixtures/std_json/corpus.txt");
    let mut inputs = Vec::new();
    let mut verdicts = Vec::new();
    for line in corpus.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (verdict, digits) = line.split_once(' ').unwrap_or((line, ""));
        let bytes = (0..digits.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&digits[at..at + 2], 16).unwrap())
            .collect::<Vec<_>>();
        assert!(matches!(verdict, "accept" | "reject"), "{line}");
        assert_eq!(
            Reference::document(&bytes, 64).is_some(),
            verdict == "accept",
            "{line}"
        );
        verdicts.push(verdict);
        inputs.push(bytes);
    }
    assert!(inputs.len() >= 60, "corpus shrank to {}", inputs.len());
    agrees_with_reference(&inputs);
}
