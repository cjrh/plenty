//! `std.csv`: bounded streaming records, dialect and limit policy, positioned
//! errors, and writers, exercised through memory cursors, scripted short
//! streams, and real files.

use std::process::{Command, Stdio};

const PRELUDE: &str = r#"import std.bytes
import std.io
import std.csv
from std.bytes import Bytes
from std.io import StreamError, Reader
from std.csv import CsvError, Dialect, Width, Newline, Quoting

# Each call consumes one step: n > 0 moves at most n bytes, 0 reports no
# progress, -1 is an interruption, -2 a failure, -3 an impossible count.
# After the script, calls move at most `rest` bytes.
pub class Fake:
    pub data: Bytes
    pub steps: list[i64]
    pub rest: i64
    pub calls: i64
    pub at: i64
    pub flushes: i64

    def step(self: &mut Fake) -> i64:
        index = self.calls
        self.calls += 1
        if index < len(self.steps):
            return self.steps[index]
        self.rest

    pub def read(self: &mut Fake, buffer: &mut Bytes, start: i64, stop: i64) -> Result[i64, StreamError]:
        step = self.step()
        if step == -1:
            return Err(StreamError.Interrupted)
        if step == -2:
            return Err(StreamError.Io(IoError.Other))
        if step == -3:
            return Ok(stop - start + 1)
        mut count = stop - start
        if count > step:
            count = step
        if count > self.data.len() - self.at:
            count = self.data.len() - self.at
        match buffer.overwrite(start, &self.data, self.at, self.at + count):
            case Ok(_):
                pass
            case Err(_):
                return Err(StreamError.OutOfRange)
        self.at += count
        Ok(count)

    pub def write(self: &mut Fake, source: &Bytes, start: i64, stop: i64) -> Result[i64, StreamError]:
        step = self.step()
        if step == -1:
            return Err(StreamError.Interrupted)
        if step == -2:
            return Err(StreamError.Io(IoError.Other))
        if step == -3:
            return Ok(stop - start + 1)
        mut count = stop - start
        if count > step:
            count = step
        match self.data.extend_range(source, start, start + count):
            case Ok(_):
                pass
            case Err(_):
                return Err(StreamError.OutOfRange)
        Ok(count)

    pub def flush(self: &mut Fake) -> Result[(), StreamError]:
        self.flushes += 1
        Ok(())

def fake(text: &str, steps: list[i64], rest: i64) -> Result[Fake, Failure]:
    Ok(Fake(std.bytes.from_text(text)?, steps, rest, 0, 0, 0))

def readable(dialect: &Dialect) -> bool:
    match std.csv.reader(std.io.stdin(), dialect, 8):
        case Ok(_):
            True
        case Err(_):
            False

def writable(dialect: &Dialect) -> bool:
    match std.csv.writer(std.io.stdout(), dialect):
        case Ok(_):
            True
        case Err(_):
            False

def loose() -> Dialect:
    mut dialect = std.csv.rfc4180()
    dialect.width = Width.Any
    dialect

# Prints every record, continuing past width errors, then the final state.
def dump[R: Reader](input: &mut std.csv.Reader[R]) -> Result[(), Failure]:
    mut record = std.csv.record()?
    mut more = True
    while more:
        outcome = input.read(&mut record)
        match outcome:
            case Ok(found):
                if found:
                    print(record.number(), record.line(), str.repr(record.texts()?)?)?
                else:
                    more = False
            case Err(cause):
                print(str.repr(&cause)?)?
                match cause:
                    case CsvError.Width(_, _, _):
                        print(record.number(), record.line(), str.repr(record.texts()?)?)?
                    case _:
                        more = False
    print("end", input.records(), input.line(), input.offset(), record.len())?
    Ok(())

def parse(text: &str, dialect: &Dialect, capacity: i64) -> Result[(), Failure]:
    mut input = std.csv.reader(std.io.cursor(std.bytes.from_text(text)?), dialect, capacity)?
    dump(&mut input)

# A summary of everything a parse produced, for comparing chunkings.
def summary(text: &str, dialect: &Dialect, capacity: i64, rest: i64) -> Result[str, Failure]:
    mut input = std.csv.reader(fake(text, []?, rest)?, dialect, capacity)?
    mut record = std.csv.record()?
    mut out = std.bytes.empty()?
    mut more = True
    while more:
        outcome = input.read(&mut record)
        match outcome:
            case Ok(found):
                if found:
                    for index in range(record.len()):
                        record.append_to(index, &mut out)?
                        out.push(124u8)?
                    out.push(59u8)?
                else:
                    more = False
            case Err(cause):
                shown = std.bytes.from_text(&str.repr(&cause)?)?
                out.extend(&shown)?
                more = False
    Ok(out.to_text()?)
"#;

fn program(body: &str) -> String {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    format!("{PRELUDE}\ndef main() -> Result[(), Failure]:\n{body}    Ok(())\n")
}

fn run(source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let output = Command::new(executable)
        .current_dir(dir.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "{source}\n{stderr}");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn native(body: &str, expected: &str) {
    assert_eq!(run(&program(body)), expected.trim_matches('\n'), "{body}");
}

fn rejected(body: &str) -> String {
    let source = program(body);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    std::fs::write(&path, &source).unwrap();
    match plenty::check_file(&path, None) {
        Ok(()) => panic!("accepted:\n{source}"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn records_end_at_lf_crlf_cr_or_end_of_input() {
    native(
        r#"
parse("a,b\nc,d\r\ne,f\rg,h", &loose(), 64)?
parse("a,b\r\n", &loose(), 64)?
parse("", &loose(), 64)?
parse("\r\n", &loose(), 64)?
"#,
        r#"
1 1 ["a", "b"]
2 2 ["c", "d"]
3 3 ["e", "f"]
4 4 ["g", "h"]
end 4 4 16 0
1 1 ["a", "b"]
end 1 2 5 0
end 0 1 0 0
1 1 [""]
end 1 2 2 0
"#,
    );
}

#[test]
fn empty_and_trailing_fields_and_blank_lines_are_kept() {
    native(
        r#"
parse(",\n,,\na,\n,a\n\n\nx", &loose(), 64)?
parse("a,", &loose(), 64)?
"#,
        r#"
1 1 ["", ""]
2 2 ["", "", ""]
3 3 ["a", ""]
4 4 ["", "a"]
5 5 [""]
6 6 [""]
7 7 ["x"]
end 7 7 14 0
1 1 ["a", ""]
end 1 1 2 0
"#,
    );
}

#[test]
fn quoted_fields_hold_delimiters_newlines_and_doubled_quotes() {
    native(
        r#"
parse("\"a,b\",\"say \"\"hi\"\"\",\"\"\n\"one\r\ntwo\nthree\rfour\",x\r\nlast,\"\"\"\"", &loose(), 64)?
parse("\"\"", &loose(), 64)?
parse("\"open\nline\"", &loose(), 64)?
"#,
        r#"
1 1 ["a,b", "say \"hi\"", ""]
2 2 ["one\r\ntwo\nthree\rfour", "x"]
3 6 ["last", "\""]
end 3 6 56 0
1 1 [""]
end 1 1 2 0
1 1 ["open\nline"]
end 1 2 11 0
"#,
    );
}

#[test]
fn syntax_errors_name_the_record_field_line_and_byte() {
    native(
        r#"
parse("a,b\nc,d\"e\nf", &loose(), 64)?
parse("a\n\"b\"c,d\n", &loose(), 64)?
parse("a,b\nc,\"d\ne", &loose(), 64)?
parse("\"a\"\"", &loose(), 64)?
"#,
        r#"
1 1 ["a", "b"]
std.csv.CsvError.Invalid(std.csv.Fault.BareQuote, std.csv.Position(record=2, field=1, line=2, offset=7))
end 1 2 7 0
1 1 ["a"]
std.csv.CsvError.Invalid(std.csv.Fault.AfterQuote, std.csv.Position(record=2, field=0, line=2, offset=5))
end 1 2 5 0
1 1 ["a", "b"]
std.csv.CsvError.Invalid(std.csv.Fault.UnterminatedQuote, std.csv.Position(record=2, field=1, line=3, offset=10))
end 1 3 10 0
std.csv.CsvError.Invalid(std.csv.Fault.UnterminatedQuote, std.csv.Position(record=1, field=0, line=1, offset=4))
end 0 1 4 0
"#,
    );
}

#[test]
fn a_failed_reader_stays_stopped() {
    native(
        r#"
mut input = std.csv.reader(std.io.cursor(std.bytes.from_text("a\"\nb\n")?), &loose(), 8)?
mut record = std.csv.record()?
print(str.repr(input.read(&mut record))?)?
print(str.repr(input.read(&mut record))?, record.len())?
print(str.repr(input.read(&mut record))?)?
"#,
        r#"
Result[bool, std.csv.CsvError].Err(std.csv.CsvError.Invalid(std.csv.Fault.BareQuote, std.csv.Position(record=1, field=0, line=1, offset=1)))
Result[bool, std.csv.CsvError].Err(std.csv.CsvError.Stopped) 0
Result[bool, std.csv.CsvError].Err(std.csv.CsvError.Stopped)
"#,
    );
}

#[test]
fn limits_bound_fields_records_and_field_counts() {
    native(
        r#"
mut dialect = loose()
dialect.max_field = 4
dialect.max_record = 6
dialect.max_fields = 3
parse("abcd,ef\nabcde\n", &dialect, 64)?
parse("abc,defg\n", &dialect, 64)?
parse("a,b,c\na,b,c,d\n", &dialect, 64)?
parse("\"ab\"\"c\",\"\"\n\"abc\"\"\"\"\"\n", &dialect, 2)?
parse("ab,\"c\nd\ne\nf\"", &dialect, 64)?
"#,
        r#"
1 1 ["abcd", "ef"]
std.csv.CsvError.Invalid(std.csv.Fault.FieldTooLong, std.csv.Position(record=2, field=0, line=2, offset=12))
end 1 2 12 0
std.csv.CsvError.Invalid(std.csv.Fault.RecordTooLong, std.csv.Position(record=1, field=1, line=1, offset=7))
end 0 1 7 0
1 1 ["a", "b", "c"]
std.csv.CsvError.Invalid(std.csv.Fault.TooManyFields, std.csv.Position(record=2, field=3, line=2, offset=13))
end 1 2 13 0
1 1 ["ab\"c", ""]
std.csv.CsvError.Invalid(std.csv.Fault.FieldTooLong, std.csv.Position(record=2, field=0, line=2, offset=18))
end 1 2 18 0
std.csv.CsvError.Invalid(std.csv.Fault.FieldTooLong, std.csv.Position(record=1, field=1, line=3, offset=8))
end 0 3 8 0
"#,
    );
}

#[test]
fn row_width_policy_reports_and_continues() {
    native(
        r#"
parse("a,b\nc\nd,e,f\ng,h\n", &std.csv.rfc4180(), 64)?
mut exact = std.csv.rfc4180()
exact.width = Width.Exactly(1)
parse("a,b\nc\n", &exact, 64)?
parse("a,b\nc\n", &loose(), 64)?
"#,
        r#"
1 1 ["a", "b"]
std.csv.CsvError.Width(2, 2, 1)
2 2 ["c"]
std.csv.CsvError.Width(3, 2, 3)
3 3 ["d", "e", "f"]
4 4 ["g", "h"]
end 4 5 16 0
std.csv.CsvError.Width(1, 1, 2)
1 1 ["a", "b"]
2 2 ["c"]
end 2 3 6 0
1 1 ["a", "b"]
2 2 ["c"]
end 2 3 6 0
"#,
    );
}

#[test]
fn dialects_choose_delimiter_and_quote_and_are_validated() {
    native(
        r#"
mut tabs = loose()
tabs.delimiter = 9u8
tabs.quote = 39u8
parse("a,b\t'c\td''e'\t\"f\"\n", &tabs, 64)?
mut bad = loose()
bad.delimiter = 34u8
print(readable(&bad), writable(&bad))?
bad.delimiter = 10u8
print(readable(&bad), writable(&bad))?
bad.delimiter = 200u8
print(readable(&bad), writable(&bad))?
bad = loose()
bad.quote = 13u8
print(readable(&bad), writable(&bad))?
bad = loose()
bad.max_field = 0
print(readable(&bad), writable(&bad))?
bad = loose()
bad.width = Width.Exactly(0)
match std.csv.writer(std.io.stdout(), &bad):
    case Ok(_):
        print("accepted")?
    case Err(cause):
        print(str.repr(cause)?)?
match std.csv.reader(std.io.stdin(), &loose(), 0):
    case Ok(_):
        print("accepted")?
    case Err(cause):
        print(str.repr(cause)?)?
"#,
        r#"
1 1 ["a,b", "c\td'e", "\"f\""]
end 1 2 17 0
False False
False False
False False
False False
False False
std.csv.CsvError.InvalidDialect
std.csv.CsvError.OutOfRange
"#,
    );
}

#[test]
fn a_byte_order_mark_is_skipped_only_on_request() {
    native(
        r#"
mut marked = loose()
marked.skip_bom = True
full = std.bytes.from_list(&[239u8, 187u8, 191u8, 34u8, 97u8, 34u8, 44u8, 98u8]?)?
part = std.bytes.from_list(&[239u8, 187u8, 97u8, 44u8, 98u8]?)?
only = std.bytes.from_list(&[239u8, 187u8, 191u8]?)?
short = std.bytes.from_list(&[239u8, 187u8]?)?
for capacity in range(1, 5):
    mut input = std.csv.reader(std.io.cursor(full.copy()?), &marked, capacity)?
    mut record = std.csv.record()?
    print(input.read(&mut record)?, record.len(), record.stop(0)?, record.text(1)?)?
mut input = std.csv.reader(std.io.cursor(part.copy()?), &marked, 2)?
mut record = std.csv.record()?
print(input.read(&mut record)?, record.len(), record.bytes(0)?.to_list()?, record.text(1)?)?
input = std.csv.reader(std.io.cursor(only.copy()?), &marked, 2)?
print(input.read(&mut record)?, input.offset())?
input = std.csv.reader(std.io.cursor(short.copy()?), &marked, 2)?
print(input.read(&mut record)?, record.bytes(0)?.to_list()?)?
input = std.csv.reader(std.io.cursor(full.copy()?), &loose(), 2)?
print(str.repr(input.read(&mut record))?)?
"#,
        r#"
True 2 1 b
True 2 1 b
True 2 1 b
True 2 1 b
True 2 [239, 187, 97] b
False 3
True [239, 187]
Result[bool, std.csv.CsvError].Err(std.csv.CsvError.Invalid(std.csv.Fault.BareQuote, std.csv.Position(record=1, field=0, line=1, offset=3)))
"#,
    );
}

#[test]
fn short_and_interrupted_reads_do_not_change_the_records() {
    native(
        r#"
mut input = std.csv.reader(fake("ab,\"c\r\nd\"\r\ne,f\r\n", [1, -1, 2, -1, -1, 1, 3]?, 1)?, &loose(), 4)?
dump(&mut input)?
print(input.inner.at)?
"#,
        r#"
1 1 ["ab", "c\r\nd"]
2 3 ["e", "f"]
end 2 4 16 0
16
"#,
    );
}

#[test]
fn stream_failures_stop_the_reader_with_the_cause() {
    native(
        r#"
mut failing = std.csv.reader(fake("a,b\nc,d\n", [5, -2]?, 8)?, &loose(), 8)?
dump(&mut failing)?
mut record = std.csv.record()?
print(str.repr(failing.read(&mut record))?)?
mut broken = std.csv.reader(fake("a,b\n", [-3]?, 8)?, &loose(), 8)?
dump(&mut broken)?
"#,
        r#"
1 1 ["a", "b"]
std.csv.CsvError.Stream(std.io.StreamError.Io(IoError.Other))
end 1 2 5 0
Result[bool, std.csv.CsvError].Err(std.csv.CsvError.Stopped)
std.csv.CsvError.Stream(std.io.StreamError.InvalidProgress)
end 0 1 0 0
"#,
    );
}

#[test]
fn every_chunking_of_the_input_gives_the_same_result() {
    native(
        r#"
mut marked = loose()
marked.skip_bom = True
marked.max_field = 9
inputs = [
    "a,b,c\r\n\"d,\"\"e\"\"\",\"f\r\ng\",\r\n\r\n,\"\"\rlast",
    "x,\"y\"\"\"\"z\",\"\"\"\"\n\"\"\"q\",\r\n\"tail\"",
    "one,two\r\rthree\n\n\"fo\"\"ur\"\r\n",
    "ok,\"row\"\nbad,\"quote\"x\n",
    "ok\nlong,\"0123\n4567\r\n89\"\n",
    "ok\r\ntruncated,\"abc\r\n",
    "wide,é,\"ü\"\n",
]?
mut mismatches = 0
mut runs = 0
for index in range(len(inputs)):
    whole = summary(&inputs[index], &marked, 256, 256)?
    print(str.repr(&whole)?)?
    for capacity in range(1, 12):
        for rest in range(1, 8):
            runs += 1
            if summary(&inputs[index], &marked, capacity, rest)? != whole:
                mismatches += 1
print(runs, mismatches)?
"#,
        r#"
"a|b|c|;d,\"e\"|f\r\ng||;|;||;last|;"
"x|y\"\"z|\"|;\"q||;tail|;"
"one|two|;|;three|;|;fo\"ur|;"
"ok|row|;std.csv.CsvError.Invalid(std.csv.Fault.AfterQuote, std.csv.Position(record=2, field=1, line=2, offset=20))"
"ok|;std.csv.CsvError.Invalid(std.csv.Fault.FieldTooLong, std.csv.Position(record=2, field=1, line=3, offset=18))"
"ok|;std.csv.CsvError.Invalid(std.csv.Fault.UnterminatedQuote, std.csv.Position(record=2, field=1, line=3, offset=20))"
"wide|é|ü|;"
539 0
"#,
    );
}

#[test]
fn lines_inside_quoted_fields_are_counted_the_same_for_every_buffer_size() {
    // CR, LF and CRLF each count once, wherever the buffer boundary falls.
    native(
        r#"
text = "\"a\r\nb\rc\nd\r\re\n\nf\n\rg\r\n\"\r\n\"\r\"\"\n\"\r\nx,\"\n\",y\r\nz"
mut lines = 0
mut starts = 0
mut mismatches = 0
for capacity in range(1, 40):
    mut input = std.csv.reader(std.io.cursor(std.bytes.from_text(text)?), &loose(), capacity)?
    mut record = std.csv.record()?
    mut seen = 0
    while input.read(&mut record)?:
        seen = seen * 100 + record.line()
    if capacity == 1:
        lines = input.line()
        starts = seen
    elif input.line() != lines or seen != starts:
        mismatches += 1
print(lines, starts, mismatches)?
"#,
        "17 1121517 0",
    );
}

#[test]
fn fields_are_read_as_ranges_copies_or_validated_text() {
    native(
        r#"
source = std.bytes.from_list(&[97u8, 98u8, 44u8, 255u8, 44u8, 44u8, 99u8, 10u8]?)?
mut input = std.csv.reader(std.io.cursor(source), &loose(), 16)?
mut record = std.csv.record()?
print(record.number(), record.len(), str.repr(record.start(0))?)?
input.read(&mut record)?
print(record.len(), record.data().len(), record.start(0)?, record.stop(0)?, record.start(3)?, record.stop(3)?)?
print(record.start(2)?, record.stop(2)?, str.repr(record.text(2)?)?)?
mut out = std.bytes.from_text(">")?
record.append_to(0, &mut out)?
record.append_to(3, &mut out)?
print(out.to_text()?, record.bytes(1)?.to_list()?, record.data().get(2)?)?
print(str.repr(record.text(1))?)?
print(str.repr(record.texts())?)?
print(str.repr(record.stop(4))?, str.repr(record.text(-1))?)?
print(str.repr(record.append_to(4, &mut out))?)?
record.clear()
print(record.len(), record.data().len(), record.number())?
"#,
        r#"
0 0 Result[i64, std.csv.CsvError].Err(std.csv.CsvError.OutOfRange)
4 4 0 2 3 4
3 3 ""
>abc [255] 255
Result[str, std.csv.CsvError].Err(std.csv.CsvError.InvalidUtf8(1, 1))
Result[list[str], std.csv.CsvError].Err(std.csv.CsvError.InvalidUtf8(1, 1))
Result[i64, std.csv.CsvError].Err(std.csv.CsvError.OutOfRange) Result[str, std.csv.CsvError].Err(std.csv.CsvError.OutOfRange)
Result[(), std.csv.CsvError].Err(std.csv.CsvError.OutOfRange)
0 0 1
"#,
    );
}

#[test]
fn writers_quote_only_what_needs_it() {
    native(
        r#"
mut out = std.csv.writer(std.io.cursor(std.bytes.empty()?), &loose())?
out.texts(&["plain", "a,b", "say \"hi\"", "two\nlines", "cr\rhere", " spaced ", ""]?)?
out.texts(&[""]?)?
out.texts(&["", ""]?)?
out.text("é")?
raw = std.bytes.from_list(&[120u8, 255u8, 34u8, 121u8]?)?
out.field(&raw, 1, 3)?
out.field(&raw, 2, 2)?
out.end_record()?
out.flush()?
print(out.records(), out.pending(), out.inner.data.len())?
print(str.repr(out.inner.data.slice(0, 65)?.to_text()?)?)?
print(out.inner.data.slice(65, out.inner.data.len())?.to_list()?)?
"#,
        r#"
4 0 76
"plain,\"a,b\",\"say \"\"hi\"\"\",\"two\nlines\",\"cr\rhere\", spaced ,\r\n\"\"\r\n,\r\n"
[195, 169, 44, 34, 255, 34, 34, 34, 44, 13, 10]
"#,
    );
}

#[test]
fn writer_dialects_choose_newline_quoting_delimiter_and_quote() {
    native(
        r#"
mut dialect = loose()
dialect.newline = Newline.Lf
dialect.quoting = Quoting.Always
dialect.delimiter = 59u8
dialect.quote = 39u8
mut out = std.csv.writer(std.io.cursor(std.bytes.empty()?), &dialect)?
out.texts(&["a", "it's", "x;y", "\"z\"", ""]?)?
out.texts(&[""]?)?
out.text("p")?
out.texts(&["q"]?)?
print(str.repr(out.inner.data.to_text()?)?)?
"#,
        r#"
"'a';'it''s';'x;y';'\"z\"';''\n''\n'p';'q'\n"
"#,
    );
}

#[test]
fn writers_reject_empty_wide_and_oversized_records() {
    native(
        r#"
mut dialect = std.csv.rfc4180()
dialect.newline = Newline.Lf
dialect.max_field = 4
dialect.max_record = 6
dialect.max_fields = 3
mut out = std.csv.writer(std.io.cursor(std.bytes.empty()?), &dialect)?
print(str.repr(out.end_record())?)?
out.texts(&["a", "b"]?)?
print(str.repr(out.texts(&["c"]?))?)?
print(str.repr(out.texts(&["d", "e", "f"]?))?)?
print(str.repr(out.text("12345"))?)?
out.text("\"\"\"\"")?
print(str.repr(out.text("123"))?)?
out.text("12")?
out.end_record()?
out.text("x")?
out.text("y")?
print(str.repr(out.text("z"))?)?
out.discard()
out.text("p")?
out.text("q")?
out.text("r")?
print(str.repr(out.text("s"))?)?
print(str.repr(out.end_record())?)?
range_error = out.field(&std.bytes.from_text("ab")?, 1, 3)
print(str.repr(range_error)?, out.records())?
print(str.repr(out.inner.data.to_text()?)?)?
mut exact = std.csv.rfc4180()
exact.width = Width.Exactly(2)
mut fixed = std.csv.writer(std.io.cursor(std.bytes.empty()?), &exact)?
print(str.repr(fixed.texts(&["a"]?))?, fixed.inner.data.len())?
fixed.text("a")?
print(str.repr(fixed.texts(&["b", "c"]?))?)?
fixed.texts(&["d", "e"]?)?
print(str.repr(fixed.inner.data.to_text()?)?)?
"#,
        r#"
Result[(), std.csv.CsvError].Err(std.csv.CsvError.EmptyRecord(1))
Result[(), std.csv.CsvError].Err(std.csv.CsvError.Width(2, 2, 1))
Result[(), std.csv.CsvError].Err(std.csv.CsvError.Width(2, 2, 3))
Result[(), std.csv.CsvError].Err(std.csv.CsvError.Rejected(std.csv.Fault.FieldTooLong, 2, 0))
Result[(), std.csv.CsvError].Err(std.csv.CsvError.Rejected(std.csv.Fault.RecordTooLong, 2, 1))
Result[(), std.csv.CsvError].Ok(())
Result[(), std.csv.CsvError].Err(std.csv.CsvError.Rejected(std.csv.Fault.TooManyFields, 3, 3))
Result[(), std.csv.CsvError].Err(std.csv.CsvError.Width(3, 2, 3))
Result[(), std.csv.CsvError].Err(std.csv.CsvError.OutOfRange) 2
"a,b\n\"\"\"\"\"\"\"\"\"\",12\n"
Result[(), std.csv.CsvError].Err(std.csv.CsvError.Width(1, 2, 1)) 0
Result[(), std.csv.CsvError].Err(std.csv.CsvError.Width(1, 2, 3))
"d,e\r\n"
"#,
    );
}

#[test]
fn writers_keep_undelivered_bytes_after_a_stream_failure() {
    native(
        r#"
mut dialect = loose()
dialect.newline = Newline.Lf
mut out = std.csv.writer(fake("", [2, -1, 1, -2, 0, -3]?, 3)?, &dialect)?
print(str.repr(out.texts(&["ab", "cd"]?))?, out.pending(), out.records(), out.inner.data.to_text()?)?
print(str.repr(out.text("ef"))?, out.pending())?
print(str.repr(out.end_record())?, out.pending())?
print(str.repr(out.flush())?, out.pending(), out.inner.data.to_text()?, out.inner.flushes)?
out.text("ef")?
out.end_record()?
out.flush()?
print(out.pending(), out.records(), str.repr(out.inner.data.to_text()?)?, out.inner.flushes)?
"#,
        r#"
Result[(), std.csv.CsvError].Err(std.csv.CsvError.Stream(std.io.StreamError.Io(IoError.Other))) 3 1 ab,
Result[(), std.csv.CsvError].Err(std.csv.CsvError.Stream(std.io.StreamError.WriteZero)) 3
Result[(), std.csv.CsvError].Err(std.csv.CsvError.Stream(std.io.StreamError.InvalidProgress)) 3
Result[(), std.csv.CsvError].Ok(()) 0 ab,cd
 1
0 2 "ab,cd\nef\n" 2
"#,
    );
}

#[test]
fn written_records_read_back_unchanged_for_random_fields() {
    native(
        r#"
alphabet = [97u8, 44u8, 34u8, 13u8, 10u8, 32u8, 59u8, 255u8, 98u8, 34u8]?
mut total = 0
mut wrong = 0
for round in range(24):
    mut dialect = loose()
    if round % 2 == 1:
        dialect.newline = Newline.Lf
    if round % 3 == 2:
        dialect.quoting = Quoting.Always
    if round % 4 == 3:
        dialect.delimiter = 59u8
    mut state = 12345 + round * 7919
    mut out = std.csv.writer(fake("", []?, 1 + round % 5)?, &dialect)?
    mut field = std.bytes.empty()?
    for row in range(40):
        state = (state * 1103515245 + 12345) % 2147483648
        width = 1 + (state // 65536) % 4
        for column in range(width):
            field.clear()
            state = (state * 1103515245 + 12345) % 2147483648
            size = (state // 65536) % 7
            for position in range(size):
                state = (state * 1103515245 + 12345) % 2147483648
                field.push(alphabet[(state // 65536) % 10])?
            out.field(&field, 0, field.len())?
        out.end_record()?
    out.flush()?
    mut input = std.csv.reader(fake("", []?, 1)?, &dialect, 1 + round % 9)?
    input.inner.data = out.inner.data.copy()?
    input.inner.rest = 1 + round % 4
    mut record = std.csv.record()?
    state = 12345 + round * 7919
    for row in range(40):
        if not input.read(&mut record)?:
            wrong += 1
        state = (state * 1103515245 + 12345) % 2147483648
        width = 1 + (state // 65536) % 4
        if record.len() != width:
            wrong += 1
        for column in range(width):
            field.clear()
            state = (state * 1103515245 + 12345) % 2147483648
            size = (state // 65536) % 7
            for position in range(size):
                state = (state * 1103515245 + 12345) % 2147483648
                field.push(alphabet[(state // 65536) % 10])?
            total += 1
            if record.bytes(column)?.to_list()? != field.to_list()?:
                wrong += 1
    if input.read(&mut record)?:
        wrong += 1
print(total, wrong)?
"#,
        "2446 0",
    );
}

#[test]
fn a_file_round_trips_through_buffered_streams() {
    native(
        r#"
dialect = std.csv.rfc4180()
mut out = std.csv.writer(std.io.buffered_writer(std.io.open_file("table.csv", "w")?, 16)?, &dialect)?
out.texts(&["id", "name", "note"]?)?
for index in range(200):
    out.text(&str.from(index)?)?
    out.text("Ada, \"the\" first")?
    out.text("line one\r\nline two")?
    out.end_record()?
out.flush()?
out.inner.inner.close()?
mut input = std.csv.reader(std.io.open_file("table.csv", "r")?, &dialect, 7)?
mut record = std.csv.record()?
mut rows = 0
mut sum = 0
input.read(&mut record)?
print(str.repr(record.texts()?)?)?
while input.read(&mut record)?:
    rows += 1
    sum += i64.parse(&record.text(0)?)?
    if record.text(1)? != "Ada, \"the\" first" or record.text(2)? != "line one\r\nline two":
        print("mismatch", record.number())?
print(rows, sum, input.records(), input.line(), record.len())?
"#,
        r#"
["id", "name", "note"]
200 19900 201 402 0
"#,
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn reading_and_writing_within_reserved_capacity_do_not_allocate() {
    native(
        r#"
dialect = std.csv.rfc4180()
mut input = std.csv.reader(std.io.cursor(std.bytes.from_text("aa,\"b,b\"\r\ncc,\"d\"\"d\"\r\nee,ff\r\n")?), &dialect, 4)?
mut out = std.csv.writer(std.io.cursor(std.bytes.with_capacity(64)?), &dialect)?
mut record = std.csv.record()?
mut kept = std.bytes.with_capacity(16)?
input.read(&mut record)?
out.record(&record)?
print("__test_fail_allocations_after_0__")?
print("__test_begin_no_allocations__")?
second = input.read(&mut record)
copied = record.append_to(1, &mut kept)
written = out.record(&record)
third = input.read(&mut record)
again = out.record(&record)
last = input.read(&mut record)
flushed = out.flush()
print("__test_restore_allocations__")?
print("__test_end_no_allocations__")?
print(str.repr(second)?, str.repr(third)?, str.repr(last)?)?
print(str.repr(copied)?, str.repr(written)?, str.repr(again)?, str.repr(flushed)?)?
print(kept.to_text()?, str.repr(out.inner.data.to_text()?)?)?
"#,
        r#"
Result[bool, std.csv.CsvError].Ok(True) Result[bool, std.csv.CsvError].Ok(True) Result[bool, std.csv.CsvError].Ok(False)
Result[(), std.csv.CsvError].Ok(()) Result[(), std.csv.CsvError].Ok(()) Result[(), std.csv.CsvError].Ok(()) Result[(), std.csv.CsvError].Ok(())
d"d "aa,\"b,b\"\r\ncc,\"d\"\"d\"\r\nee,ff\r\n"
"#,
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_is_reported_and_leaves_writers_consistent() {
    native(
        r#"
dialect = loose()
mut input = std.csv.reader(std.io.cursor(std.bytes.from_text("a,b\nc,d\n")?), &dialect, 8)?
mut record = std.csv.record()?
mut filled = std.csv.record()?
mut other = std.csv.reader(std.io.cursor(std.bytes.from_text("x,y\n")?), &dialect, 8)?
other.read(&mut filled)?
mut out = std.csv.writer(std.io.cursor(std.bytes.empty()?), &dialect)?
field = std.bytes.from_text("p,q")?
print("__test_fail_allocations_after_0__")?
a = readable(&dialect)
b = input.read(&mut record)
c = input.read(&mut record)
d = filled.text(0)
e = filled.texts()
f = out.field(&field, 0, 3)
g = out.text("z")
h = out.end_record()
print("__test_restore_allocations__")?
print(a, str.repr(b)?, str.repr(c)?)?
print(str.repr(d)?)?
print(str.repr(e)?)?
print(str.repr(f)?, str.repr(g)?, str.repr(h)?)?
out.field(&field, 0, 3)?
out.end_record()?
print(str.repr(out.inner.data.to_text()?)?, out.records())?
"#,
        r#"
False Result[bool, std.csv.CsvError].Err(std.csv.CsvError.Allocation(AllocError.OutOfMemory)) Result[bool, std.csv.CsvError].Err(std.csv.CsvError.Stopped)
Result[str, std.csv.CsvError].Err(std.csv.CsvError.Allocation(AllocError.OutOfMemory))
Result[list[str], std.csv.CsvError].Err(std.csv.CsvError.Allocation(AllocError.OutOfMemory))
Result[(), std.csv.CsvError].Err(std.csv.CsvError.Allocation(AllocError.OutOfMemory)) Result[(), std.csv.CsvError].Err(std.csv.CsvError.Allocation(AllocError.OutOfMemory)) Result[(), std.csv.CsvError].Err(std.csv.CsvError.EmptyRecord(1))
"\"p,q\"\r\n" 1
"#,
    );
}

#[test]
fn a_view_of_a_record_cannot_outlive_the_next_read() {
    let error = rejected(
        r#"
mut input = std.csv.reader(std.io.cursor(std.bytes.from_text("a\nb\n")?), &loose(), 8)?
mut record = std.csv.record()?
input.read(&mut record)?
view = record.data()
input.read(&mut record)?
print(view.len())?
"#,
    );
    assert!(error.contains("borrow"), "{error}");
}

#[test]
fn reader_and_record_state_is_private() {
    let error = rejected(
        r#"
mut record = std.csv.record()?
record._ends.append(4)?
"#,
    );
    assert!(error.contains("private"), "{error}");
    let error = rejected(
        r#"
mut input = std.csv.reader(std.io.stdin(), &loose(), 8)?
input._stopped = False
"#,
    );
    assert!(error.contains("private"), "{error}");
}

#[test]
fn readers_and_writers_need_matching_streams() {
    let error = rejected(
        r#"
mut input = std.csv.reader(std.io.stdout(), &loose(), 8)?
mut record = std.csv.record()?
input.read(&mut record)?
"#,
    );
    assert!(error.contains("read"), "{error}");
    let error = rejected(
        r#"
mut out = std.csv.writer(std.io.stdin(), &loose())?
out.text("a")?
"#,
    );
    assert!(error.contains("write"), "{error}");
}

#[test]
fn an_unknown_module_lists_std_csv() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    std::fs::write(&path, "import std.nothing\n\ndef main() -> ():\n    pass\n").unwrap();
    let error = plenty::check_file(&path, None).unwrap_err().to_string();
    assert!(error.contains("std.csv"), "{error}");
}
