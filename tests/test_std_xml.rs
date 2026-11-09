//! Bounded XML events, stream progress, and XML 1.0 well-formedness.
mod support;
use std::process::Command;

const PRELUDE: &str = r#"import std.xml
import std.bytes
import std.io
from std.xml import Event, Attribute, Fault, Limits
from std.bytes import Bytes
from std.io import StreamError

def min(a: i64, b: i64) -> i64:
    a if a < b else b

pub class Trickle:
    data: Bytes
    at: i64
    step: i64
    mode: i64

    pub def read(self: &mut Trickle, target: &mut Bytes, start: i64, stop: i64) -> Result[i64, StreamError]:
        mode = self.mode
        self.mode = 0
        if mode == 1:
            return Err(StreamError.Interrupted)
        if mode == 2:
            return Err(StreamError.UnexpectedEof)
        if mode == 3:
            lie = checked:
                stop - start + 1
            return lie.map_error(def(e: OverflowError[i64]) -> StreamError: StreamError.OutOfRange)
        if start < 0 or stop < start or stop > target.len() or self.at < 0 or self.at > self.data.len() or self.step < 0:
            return Err(StreamError.InvalidProgress)
        # Ordered endpoints; count <= remaining input and output span.
        unchecked:
            count = min(min(self.step, stop - start), self.data.len() - self.at)
            target.overwrite(start, &self.data[self.at:self.at + count])
            self.at += count
            Ok(count)

pub class Sink:
    data: Bytes
    room: i64
    fail_flush: bool

    pub def write(self: &mut Sink, source: &[u8]) -> Result[i64, StreamError]:
        if self.room < 0:
            return Err(StreamError.InvalidProgress)
        count = min(min(2, self.room), len(source))
        match self.data.extend(&source[:count]):
            case Err(cause):
                return Err(StreamError.Allocation(cause))
            case Ok(_):
                pass
        # count <= the nonnegative remaining room.
        unchecked:
            self.room -= count
        Ok(count)

    pub def flush(self: &mut Sink) -> Result[(), StreamError]:
        if self.fail_flush:
            Err(StreamError.UnexpectedEof)
        else:
            Ok(())

def parse(text: str, step: i64, bounds: &Limits) -> Result[i64, AllocError | std.xml.XmlError | OverflowError[i64]]:
    mut reader = std.xml.reader(Trickle(std.bytes.from_text(&text)?, 0, step, 1), bounds)?
    mut count = 0
    while True:
        match reader.next()?:
            case Event.Eof:
                return Ok(count)
            case _:
                count += 1

def fault(text: str, bounds: &Limits) -> Fault:
    match parse(text, 1, bounds):
        case Ok(_):
            Fault.InvalidState
        case Err(error):
            match error:
                case cause: std.xml.XmlError:
                    cause.fault
                case _:
                    Fault.InvalidState
"#;

fn native(body: &str) -> String {
    let body = body
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    let source = format!(
        "{PRELUDE}\ndef main() -> Result[(), AllocError | IoError | std.xml.XmlError | std.bytes.BytesError | OverflowError[i64]]:\n{body}    Ok(())\n"
    );
    output_with_optimization(&source, false)
}

fn output_with_optimization(source: &str, optimize: bool) -> String {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&input, source).unwrap();
    let options = plenty::CompileOptions {
        optimize,
        test_hooks: true,
        ..Default::default()
    };
    plenty::compile_file_to_executable_with_options(&input, &executable, None, &options)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let output = Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn events_normalize_lines_and_decode_only_allowed_entities() {
    let result = native(
        r#"
text = (chr(65279u32).unwrap() + "<?xml version='1.0' encoding='utf-8' standalone='yes'?><!--ok--><?go data?><é x='a\r\nb\t&#9;&amp;' xmlns:p='urn:x'>\r\n&lt;&#x1F600;<![CDATA[a<b]]><p:z/></é>")?
for step in [1, 2, 7, 4096]?:
    print(parse(text, step, &std.xml.limits())?)?
mut r = std.xml.reader(Trickle(std.bytes.from_text(&text)?, 0, 1, 1), &std.xml.limits())?
for _ in range(10):
    print(r.next()?)?
print(r.offset(), text.byte_len())?
"#,
    );
    assert!(result.starts_with("8\n8\n8\n8\n"), "{result}");
    assert!(
        result.contains("Declaration(Option[bool].Some(True))"),
        "{result}"
    );
    assert!(result.contains(r#"name="x", value="a b \t&""#), "{result}");
    assert!(result.contains(r#"Text("\n<😀")"#), "{result}");
    assert!(result.contains(r#"CData("a<b")"#), "{result}");
    let last = result
        .lines()
        .last()
        .unwrap()
        .split_whitespace()
        .collect::<Vec<_>>();
    assert_eq!(last[0], last[1]);
}

#[test]
fn rejects_malformed_documents_and_external_entities() {
    let cases = [
        ("", "UnexpectedEnd"),
        ("<a>", "UnexpectedEnd"),
        ("<a></b>", "MismatchedEnd"),
        ("<a/><b/>", "Syntax"),
        ("text<a/>", "Syntax"),
        ("&#32;<a/>", "Syntax"),
        (
            "<!DOCTYPE a SYSTEM 'file:///etc/passwd'><a/>",
            "DoctypeForbidden",
        ),
        (
            "<!DOCTYPE a [<!ENTITY x 'boom'>]><a>&x;</a>",
            "DoctypeForbidden",
        ),
        ("<a>&missing;</a>", "UnknownEntity"),
        ("<a>&#0;</a>", "InvalidReference"),
        ("<a>&#xD800;</a>", "InvalidReference"),
        ("<a>&#1114112;</a>", "InvalidReference"),
        ("<a>&#X41;</a>", "InvalidReference"),
        ("<a>&amp</a>", "InvalidReference"),
        ("<a>]]></a>", "Syntax"),
        ("<a x='<a'/>", "Syntax"),
        ("<a x='1' x='2'/>", "DuplicateAttribute"),
        ("<1a/>", "InvalidName"),
        ("<a><!--bad--comment--></a>", "Syntax"),
        (" <a/><?xml version='1.0'?>", "UnsupportedDeclaration"),
        ("<?xml version='1.1'?><a/>", "UnsupportedDeclaration"),
        (
            "<?xml version='1.0' encoding='ISO-8859-1'?><a/>",
            "UnsupportedDeclaration",
        ),
        ("<?XML version='1.0'?><a/>", "UnsupportedDeclaration"),
    ];
    let body = cases
        .iter()
        .map(|(input, _)| format!("print(fault({input:?}, &std.xml.limits()))?\n"))
        .collect::<String>();
    let expected = cases
        .iter()
        .map(|(_, kind)| format!("std.xml.Fault.{kind}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(native(&body), expected);
}

#[test]
fn writer_escapes_data_and_validates_structure() {
    let result = native(
        r#"
mut w = std.xml.writer(Sink(std.bytes.empty()?, 10000, False), &std.xml.limits())?
w.write(&Event.Declaration(Some(False)))?
w.write(&Event.Text("\r\n"))?
w.write(&Event.Start("root", [Attribute("a", "<&\"\r\n\t")]?))?
w.write(&Event.Text("<&>\r"))?
w.write(&Event.Empty("child", list[Attribute]()?))?
w.write(&Event.CData("raw<>&"))?
w.write(&Event.Comment("hi"))?
w.write(&Event.ProcessingInstruction("go", "now"))?
w.write(&Event.End("root"))?
w.write(&Event.Eof)?
w.finish()?
text = w.inner.data.to_text()?
print(text)?
print(parse(text, 1, &std.xml.limits())?)?
print(w.offset(), text.byte_len())?
match w.write(&Event.Text("x")):
    case Err(error):
        print(error.fault)?
    case Ok(_):
        print("bad")?
"#,
    );
    assert!(
        result.contains(r#"a="&lt;&amp;&quot;&#13;&#10;&#9;""#),
        "{result}"
    );
    assert!(result.contains("&lt;&amp;&gt;&#13;<child/>"), "{result}");
    assert!(result.contains("\n9\n"), "{result}");
    assert!(result.ends_with("std.xml.Fault.InvalidState"), "{result}");
}

#[test]
fn resource_bounds_and_stopped_states_are_explicit() {
    let result = native(
        r#"
b = std.xml.limits()
print(parse("<a/>", 1, &Limits(1, 4, 4, 4, 0))?)?
print(fault("<a/>", &Limits(0, 4, 4, 4, 0)))?
print(fault("<a/>", &Limits(1, 3, 4, 4, 0)))?
print(fault("<a/>", &Limits(1, 4, 4, 3, 0)))?
print(fault("<a x='1'/>", &Limits(1, 50, 50, 50, 0)))?
mut r = std.xml.reader(Trickle(std.bytes.from_text(&"<a></b>")?, 0, 1, 0), &b)?
drop(r.next())
drop(r.next())
match r.next():
    case Err(error):
        print(error.fault)?
    case Ok(_):
        print("bad")?
mut w = std.xml.writer(Sink(std.bytes.empty()?, 100, False), &Limits(1, 50, 3, 50, 0))?
match w.write(&Event.Empty("a", list[Attribute]()?)):
    case Err(error):
        print(error.fault, error.offset)?
    case Ok(_):
        print("bad")?
print(w.inner.data.len())?
"#,
    );
    assert_eq!(
        result,
        "1\nstd.xml.Fault.DepthExceeded\nstd.xml.Fault.InputTooLarge\nstd.xml.Fault.TokenTooLarge\nstd.xml.Fault.TooManyAttributes\nstd.xml.Fault.Stopped\nstd.xml.Fault.OutputTooLarge 0\n0"
    );
}

#[test]
fn stream_errors_preserve_completed_output_and_detect_invalid_reads() {
    let result = native(
        r#"
for mode in [2, 3]?:
    mut r = std.xml.reader(Trickle(std.bytes.from_text(&"<a/>")?, 0, 1, mode), &std.xml.limits())?
    match r.next():
        case Err(error):
            print(error.fault, error.offset)?
        case Ok(_):
            print("bad")?
for room in [2, 100]?:
    mut w = std.xml.writer(Sink(std.bytes.empty()?, room, True), &std.xml.limits())?
    result = w.write(&Event.Empty("a", list[Attribute]()?))
    match result:
        case Err(error):
            print(error.fault, error.offset)?
        case Ok(_):
            match w.finish():
                case Err(error):
                    print(error.fault, error.offset)?
                case Ok(_):
                    print("bad")?
"#,
    );
    assert_eq!(
        result,
        "std.xml.Fault.Stream(std.io.StreamError.UnexpectedEof) 0\nstd.xml.Fault.Stream(std.io.StreamError.InvalidProgress) 0\nstd.xml.Fault.Stream(std.io.StreamError.WriteZero) 2\nstd.xml.Fault.Stream(std.io.StreamError.UnexpectedEof) 4"
    );
}

#[test]
fn document_streams_beyond_buffer_size_with_small_token_bound() {
    let result = native(
        r#"
mut pieces = list[str]()?
pieces.append("<root>")?
for _ in range(3000):
    pieces.append("<x/>")?
pieces.append("</root>")?
print(parse("".join(&pieces)?, 7, &Limits(2, 20000, 20000, 7, 0))?)?
mut data = std.bytes.from_list(&[60u8, 97u8, 62u8, 255u8, 60u8, 47u8, 97u8, 62u8]?)?
mut r = std.xml.reader(std.io.cursor(data), &std.xml.limits())?
drop(r.next())
match r.next():
    case Err(error):
        print(error.fault)?
    case Ok(_):
        print("bad")?
"#,
    );
    assert_eq!(result, "3002\nstd.xml.Fault.InvalidUtf8");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_is_returned_and_terminal() {
    let result = native(
        r#"
mut r = std.xml.reader(Trickle(std.bytes.from_text(&"<long_element attribute='long value'/>")?, 0, 1, 0), &std.xml.limits())?
mut w = std.xml.writer(Sink(std.bytes.empty()?, 1000, False), &std.xml.limits())?
event = Event.Empty("long_element", list[Attribute]()?)
print("__test_fail_allocations_after_0__")?
a = r.next()
b = w.write(&event)
print("__test_restore_allocations__")?
match a:
    case Err(error):
        print(error.fault)?
    case Ok(_):
        print("bad")?
match b:
    case Err(error):
        print(error.fault)?
    case Ok(_):
        print("bad")?
match r.next():
    case Err(error):
        print(error.fault)?
    case Ok(_):
        print("bad")?
"#,
    );
    assert_eq!(
        result,
        "std.xml.Fault.Allocation(AllocError.OutOfMemory)\nstd.xml.Fault.Allocation(AllocError.OutOfMemory)\nstd.xml.Fault.Stopped"
    );
}

#[test]
fn writer_rejects_invalid_events_before_emitting_bytes() {
    let result = native(
        r#"
for event in [Event.End("a"), Event.CData("outside"), Event.Text("outside"), Event.Comment("bad--comment"), Event.ProcessingInstruction("xml", "x"), Event.Empty("1bad", list[Attribute]()?), Event.Empty("a", [Attribute("x", "1"), Attribute("x", "2")]?)]?:
    mut w = std.xml.writer(Sink(std.bytes.empty()?, 1000, False), &std.xml.limits())?
    match w.write(&event):
        case Err(error):
            print(error.fault, w.offset(), w.inner.data.len())?
        case Ok(_):
            print("bad")?
mut w = std.xml.writer(Sink(std.bytes.empty()?, 1000, False), &Limits(1, 100, 100, 4, 0))?
match w.write(&Event.Empty("abc", list[Attribute]()?)):
    case Err(error):
        print(error.fault, w.offset(), w.inner.data.len())?
    case Ok(_):
        print("bad")?
mut unfinished = std.xml.writer(Sink(std.bytes.empty()?, 1000, False), &std.xml.limits())?
unfinished.write(&Event.Start("a", list[Attribute]()?))?
match unfinished.finish():
    case Err(error):
        print(error.fault, error.offset)?
    case Ok(_):
        print("bad")?
"#,
    );
    assert_eq!(
        result,
        "std.xml.Fault.MismatchedEnd 0 0\nstd.xml.Fault.InvalidState 0 0\nstd.xml.Fault.InvalidState 0 0\nstd.xml.Fault.Syntax 0 0\nstd.xml.Fault.Syntax 0 0\nstd.xml.Fault.InvalidName 0 0\nstd.xml.Fault.DuplicateAttribute 0 0\nstd.xml.Fault.TokenTooLarge 0 0\nstd.xml.Fault.UnexpectedEnd 3"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_at_each_stage_remains_an_explicit_error() {
    let result = native(
        r#"
mut failures = 0
mut successes = 0
for budget in range(200):
    mut r = std.xml.reader(Trickle(std.bytes.from_text(&"<long_name attribute='long &amp; value'>some &#x1F600; text</long_name>")?, 0, 1, 0), &std.xml.limits())?
    mut w = std.xml.writer(Sink(std.bytes.empty()?, 10000, False), &std.xml.limits())?
    marker = ("__test_fail_allocations_after_" + str.from(budget)?)?
    print((marker + "__")?)?
    mut failed = False
    mut valid = True
    mut done = False
    while not done:
        match r.next():
            case Err(error):
                failed = True
                done = True
                match error.fault:
                    case Fault.Allocation(_):
                        pass
                    case _:
                        valid = False
            case Ok(event):
                match &event:
                    case Event.Eof:
                        done = True
                    case _:
                        pass
                match w.write(&event):
                    case Ok(_):
                        pass
                    case Err(error):
                        failed = True
                        done = True
                        match error.fault:
                            case Fault.Allocation(_):
                                pass
                            case Fault.Stream(cause):
                                match cause:
                                    case StreamError.Allocation(_):
                                        pass
                                    case _:
                                        valid = False
                            case _:
                                valid = False
    print("__test_restore_allocations__")?
    if not valid:
        print("bad error", budget)?
    if failed:
        failures += 1
    else:
        successes += 1
print(failures > 0, successes > 0)?
"#,
    );
    assert_eq!(result, "True True");
}

#[test]
fn xml_character_ranges_are_checked() {
    let result = native(
        r#"
for code in [65536u32, 983039u32, 983040u32]?:
    name = chr(code).unwrap()
    document = ("<" + name)?
    print(parse((document + "/>")?, 1, &std.xml.limits()).is_ok())?
for code in [0u32, 1u32, 65534u32, 65535u32]?:
    document = ("<a>" + chr(code).unwrap())?
    print(fault((document + "</a>")?, &std.xml.limits()))?
"#,
    );
    let expected = format!(
        "True\nTrue\nFalse\n{}",
        ["std.xml.Fault.InvalidCharacter"; 4].join("\n"),
    );
    assert_eq!(result, expected);
}

#[test]
fn a_depth_limit_above_the_ceiling_traps() {
    for call in [
        "drop(std.xml.reader(std.io.cursor(std.bytes.empty()?), &bounds))",
        "drop(std.xml.writer(std.io.cursor(std.bytes.empty()?), &bounds))",
    ] {
        let source = format!(
            "{PRELUDE}\ndef main() -> Result[(), AllocError]:\n    bounds = Limits(NonNegative.try_from(1025).unwrap(), 5, 5, 5, 0)\n    {call}\n    Ok(())\n"
        );
        support::assert_file_traps(&source, "std.xml: max_depth is above 1024");
    }
}

#[test]
fn out_of_domain_limits_are_compile_errors() {
    // `max_token` is `Positive[i64]` and the rest `NonNegative[i64]`, replacing
    // the old `InvalidLimits` cases.
    for (bounds, message) in [
        ("Limits(1, 5, 5, 0, 0)", "must be greater than zero"),
        ("Limits(-1, 5, 5, 5, 0)", "cannot be negative"),
        ("Limits(1, 5, 5, 5, -1)", "cannot be negative"),
    ] {
        let source = format!("{PRELUDE}\ndef main() -> ():\n    drop({bounds})\n");
        let error = support::check_file_error(&source);
        assert!(error.contains(message), "{error}");
    }
}

#[test]
fn escaped_output_is_bounded_after_expansion() {
    let result = native(
        r#"
mut w = std.xml.writer(Sink(std.bytes.empty()?, 1000, False), &Limits(2, 1000, 1000, 5, 3))?
w.write(&Event.Start("a", list[Attribute]()?))?
match w.write(&Event.Text("&&")):
    case Err(error):
        print(error.fault, error.offset, w.inner.data.to_text()?)?
    case Ok(_):
        print("bad")?
"#,
    );
    assert_eq!(result, "std.xml.Fault.TokenTooLarge 3 <a>");
}

#[test]
fn reference_and_raw_preserve_scalar_token_and_input_faults_at_representation_limits() {
    let library = include_str!("../stdlib/std/xml.plenty");
    let helpers =
        &library[library.find("def allocated[").unwrap()..library.find("def space(").unwrap()];
    let xml_char =
        &library[library.find("def xml_char(").unwrap()..library.find("def name_start(").unwrap()];
    let reference =
        &library[library.find("def reference(").unwrap()..library.find("def unescape(").unwrap()];
    let reader = &library
        [library.find("pub class Reader[").unwrap()..library.find("    def take(").unwrap()];
    let constructor = &library
        [library.find("pub def reader[").unwrap()..library.find("# The builder checks").unwrap()];
    let emit = &library
        [library.find("    def emit(").unwrap()..library.find("    pub def write(").unwrap()];
    let sink =
        &PRELUDE[PRELUDE.find("pub class Sink:").unwrap()..PRELUDE.find("def parse(").unwrap()];
    let minimum =
        &PRELUDE[PRELUDE.find("def min(").unwrap()..PRELUDE.find("pub class Trickle:").unwrap()];
    let source = format!(
        r##"import std.xml
import std.bytes
import std.io
from std.xml import Limits, Fault, XmlError, Event
from std.bytes import Bytes, BytesError
from std.io import StreamError
{helpers}
{xml_char}
{reference}
{reader}
def check_depth(bounds: &Limits) -> ():
    pass
{constructor}
{minimum}
{sink}
class Writer[W]:
    inner: W
    _limits: Limits
    _offset: i64

    # Keep the actual production emit path, with a fixed two-byte encoded event.
    def encode(self: &mut Writer[W], event: &Event) -> Result[Bytes, Fault]:
        allocated(std.bytes.from_text("xx"))
{emit}
def main() -> Result[(), AllocError | IoError | XmlError]:
    bounds = Limits(64, NonNegative.try_from(i64.MAX).unwrap(), NonNegative.try_from(i64.MAX).unwrap(), 1, 4)
    mut input = reader(std.io.cursor(std.bytes.from_text("xy")?), &bounds)?
    input._offset = 9223372036854775806
    input._token_start = 9223372036854775806
    mut token = reader(std.io.cursor(std.bytes.from_text("xy")?), &bounds)?
    token._offset = 9223372036854775805
    token._token_start = 9223372036854775805
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    first = input.raw().unwrap() == Some(120u8)
    input_fault = match input.raw():
        case Err(Fault.InputTooLarge):
            True
        case _:
            False
    exact = input.offset() == i64.MAX and input._pos == 1
    one = token.raw().is_ok()
    token_fault = match token.raw():
        case Err(Fault.TokenTooLarge):
            True
        case _:
            False
    kept = token.offset() == 9223372036854775806 and token._pos == 1
    print("__test_restore_allocations__")?
    print("__test_end_no_allocations__")?
    print(first, input_fault, exact, one, token_fault, kept)?
    mut complete = Writer(Sink(std.bytes.empty()?, 8, False), copy(bounds)?, 9223372036854775805)
    print(complete.emit(&Event.Text("")).is_ok(), complete._offset == i64.MAX)?
    rejected = match complete.emit(&Event.Text("")):
        case Err(Fault.OutputTooLarge):
            True
        case _:
            False
    print(rejected, complete.inner.data.len() == 2)?
    mut partial = Writer(Sink(std.bytes.empty()?, 1, False), bounds, 9223372036854775805)
    short = match partial.emit(&Event.Text("")):
        case Err(Fault.Stream(StreamError.WriteZero)):
            True
        case _:
            False
    print(short, partial._offset == 9223372036854775806, partial.inner.data.len() == 1)?
    print(reference("#x10FFFF").is_ok(), reference("#1114111").is_ok())?
    for text in ["#x110000", "#1114112", "#9999999999999999999999999", "#xFFFFFFFF", "#xD800", "#0"]?:
        invalid = match reference(text):
            case Err(Fault.InvalidReference):
                True
            case _:
                False
        print(invalid)?
    Ok(())
"##
    );
    for optimize in [false, true] {
        assert_eq!(
            output_with_optimization(&source, optimize),
            "True True True True True True\nTrue True\nTrue True\nTrue True True\nTrue True\nTrue\nTrue\nTrue\nTrue\nTrue\nTrue"
        );
    }
}
