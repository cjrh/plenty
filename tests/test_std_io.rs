//! `std.io`: partial-progress stream contracts, retry helpers, and bounded
//! buffering, exercised through scripted fake streams and real descriptors.

use std::io::Write;
use std::process::{Command, Stdio};

const PRELUDE: &str = r#"import std.bytes
import std.io
from std.bytes import ByteSource, Bytes
from std.io import StreamError, SeekFrom, Partial, CopyError, CloseError

# Each call consumes one step: n > 0 moves at most n bytes, 0 reports no
# progress, -1 is an interruption, -2 a failure, -3 an impossible count.
# After the script, calls move everything offered.
pub class Fake:
    pub data: Bytes
    pub steps: list[i64]
    pub calls: i64
    pub at: i64
    pub flushes: i64
    pub closes: i64
    pub fail_flush: bool
    pub fail_close: bool

    def step(self: &mut Fake) -> i64:
        index = self.calls
        self.calls += 1
        if index < len(self.steps):
            return self.steps[index]
        i64.MAX

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

    pub def write[S: ByteSource](self: &mut Fake, source: &S, start: i64, stop: i64) -> Result[i64, StreamError]:
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
        if self.fail_flush:
            return Err(StreamError.Io(IoError.NotWritable))
        Ok(())

    pub def close(self: &mut Fake) -> Result[(), StreamError]:
        self.closes += 1
        if self.fail_close:
            return Err(StreamError.Io(IoError.Other))
        Ok(())

def fake(text: &str, steps: list[i64]) -> Result[Fake, AllocError | IoError | std.bytes.BytesError]:
    Ok(Fake(std.bytes.from_text(text)?, steps, 0, 0, 0, 0, False, False))

def blank(count: i64) -> Result[Bytes, AllocError | IoError | std.bytes.BytesError]:
    mut buffer = std.bytes.with_capacity(count)?
    buffer.resize(count, 46u8)?
    Ok(buffer)
"#;

fn program(body: &str) -> String {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    format!("{PRELUDE}\ndef main() -> Result[(), AllocError | IoError | std.bytes.BytesError | std.io.StreamError]:\n{body}    Ok(())\n")
}

struct Run {
    stdout: String,
    stderr: String,
}

fn run(source: &str, input: &[u8]) -> Run {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let mut child = Command::new(executable)
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "{source}\n{stderr}");
    let stdout = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n");
    Run { stdout, stderr }
}

fn native(body: &str, expected: &str) {
    assert_eq!(
        run(&program(body), b"").stdout,
        expected.trim_matches('\n'),
        "{body}"
    );
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
fn read_exact_retries_short_and_interrupted_reads() {
    native(
        r#"
mut reader = fake("abcdefgh", [2, -1, 1, -1, -1, 3]?)?
mut buffer = blank(8)?
print(str.repr(std.io.read_exact(&mut reader, &mut buffer, 1, 7))?)?
print(buffer.to_text()?, reader.calls, reader.at)?
"#,
        r#"
Result[(), std.io.Partial].Ok(())
.abcdef. 6 6
"#,
    );
}

#[test]
fn read_exact_reports_progress_before_end_of_input_and_failure() {
    native(
        r#"
mut short = fake("abc", [2]?)?
mut buffer = blank(6)?
print(str.repr(std.io.read_exact(&mut short, &mut buffer, 0, 6))?)?
print(buffer.to_text()?)?
mut failing = fake("abcdef", [2, 2, -2]?)?
print(str.repr(std.io.read_exact(&mut failing, &mut buffer, 0, 6))?)?
mut lying = fake("abcdef", [1, -3]?)?
print(str.repr(std.io.read_exact(&mut lying, &mut buffer, 0, 6))?)?
mut untouched = fake("abcdef", []?)?
print(str.repr(std.io.read_exact(&mut untouched, &mut buffer, 2, 7))?)?
print(str.repr(std.io.read_exact(&mut untouched, &mut buffer, 3, 3))?)?
print(untouched.calls)?
"#,
        r#"
Result[(), std.io.Partial].Err(std.io.Partial(completed=3, cause=std.io.StreamError.UnexpectedEof))
abc...
Result[(), std.io.Partial].Err(std.io.Partial(completed=4, cause=std.io.StreamError.Io(IoError.Other)))
Result[(), std.io.Partial].Err(std.io.Partial(completed=1, cause=std.io.StreamError.InvalidProgress))
Result[(), std.io.Partial].Err(std.io.Partial(completed=0, cause=std.io.StreamError.OutOfRange))
Result[(), std.io.Partial].Ok(())
0
"#,
    );
}

#[test]
fn write_all_retries_short_and_interrupted_writes() {
    native(
        r#"
source = std.bytes.from_text("abcdefgh")?
mut writer = fake("", [3, -1, 1, -1, 2]?)?
print(str.repr(std.io.write_all(&mut writer, &source, 1, 8))?)?
print(writer.data.to_text()?, writer.calls, writer.flushes)?
"#,
        r#"
Result[(), std.io.Partial].Ok(())
bcdefgh 6 0
"#,
    );
}

#[test]
fn write_all_reports_progress_before_zero_progress_and_failure() {
    native(
        r#"
source = std.bytes.from_text("abcdefgh")?
mut stuck = fake("", [3, 0]?)?
print(str.repr(std.io.write_all(&mut stuck, &source, 0, 8))?)?
print(stuck.data.to_text()?)?
mut failing = fake("", [2, 3, -2]?)?
print(str.repr(std.io.write_all(&mut failing, &source, 0, 8))?)?
print(failing.data.to_text()?)?
mut lying = fake("", [-3]?)?
print(str.repr(std.io.write_all(&mut lying, &source, 0, 8))?)?
mut untouched = fake("", []?)?
print(str.repr(std.io.write_all(&mut untouched, &source, 4, 9))?)?
print(str.repr(std.io.write_all(&mut untouched, &source, 4, 4))?)?
print(untouched.calls)?
"#,
        r#"
Result[(), std.io.Partial].Err(std.io.Partial(completed=3, cause=std.io.StreamError.WriteZero))
abc
Result[(), std.io.Partial].Err(std.io.Partial(completed=5, cause=std.io.StreamError.Io(IoError.Other)))
abcde
Result[(), std.io.Partial].Err(std.io.Partial(completed=0, cause=std.io.StreamError.InvalidProgress))
Result[(), std.io.Partial].Err(std.io.Partial(completed=0, cause=std.io.StreamError.OutOfRange))
Result[(), std.io.Partial].Ok(())
0
"#,
    );
}

#[test]
fn read_up_to_appends_no_more_than_the_limit() {
    native(
        r#"
mut reader = fake("abcdefgh", [3, -1, 1]?)?
mut out = std.bytes.from_text("->")?
print(str.repr(std.io.read_up_to(&mut reader, &mut out, 5))?)?
print(out.to_text()?, out.len())?
print(str.repr(std.io.read_up_to(&mut reader, &mut out, 100))?)?
print(out.to_text()?, out.len())?
print(str.repr(std.io.read_up_to(&mut reader, &mut out, 100))?)?
print(str.repr(std.io.read_up_to(&mut reader, &mut out, 0))?)?
print(str.repr(std.io.read_up_to(&mut reader, &mut out, -1))?)?
print(out.len())?
"#,
        r#"
Result[i64, std.io.Partial].Ok(5)
->abcde 7
Result[i64, std.io.Partial].Ok(3)
->abcdefgh 10
Result[i64, std.io.Partial].Ok(0)
Result[i64, std.io.Partial].Ok(0)
Result[i64, std.io.Partial].Err(std.io.Partial(completed=0, cause=std.io.StreamError.OutOfRange))
10
"#,
    );
}

#[test]
fn read_up_to_keeps_only_delivered_bytes_after_a_failure() {
    native(
        r#"
mut failing = fake("abcdefgh", [3, -2]?)?
mut out = std.bytes.empty()?
print(str.repr(std.io.read_up_to(&mut failing, &mut out, 8))?)?
print(out.to_text()?, out.len())?
mut lying = fake("abcdefgh", [2, -3]?)?
mut other = std.bytes.empty()?
print(str.repr(std.io.read_up_to(&mut lying, &mut other, 8))?)?
print(other.to_text()?, other.len())?
"#,
        r#"
Result[i64, std.io.Partial].Err(std.io.Partial(completed=3, cause=std.io.StreamError.Io(IoError.Other)))
abc 3
Result[i64, std.io.Partial].Err(std.io.Partial(completed=2, cause=std.io.StreamError.InvalidProgress))
ab 2
"#,
    );
}

#[test]
fn copy_all_moves_everything_through_caller_scratch() {
    native(
        r#"
mut reader = fake("the quick brown fox", [5, -1, 2]?)?
mut writer = fake("", [1, -1, 2]?)?
mut scratch = blank(4)?
print(str.repr(std.io.copy_all(&mut reader, &mut writer, &mut scratch))?)?
print(writer.data.to_text()?, writer.flushes, scratch.len())?
"#,
        r#"
Result[i64, std.io.CopyError].Ok(19)
the quick brown fox 0 4
"#,
    );
}

#[test]
fn copy_all_names_the_failing_side_and_the_delivered_count() {
    native(
        r#"
mut scratch = blank(4)?
mut reader = fake("abcdefghijkl", [4, 4, -2]?)?
mut writer = fake("", []?)?
print(str.repr(std.io.copy_all(&mut reader, &mut writer, &mut scratch))?)?
print(writer.data.to_text()?)?
mut source = fake("abcdefghijkl", []?)?
mut failing = fake("", [4, 3, -2]?)?
print(str.repr(std.io.copy_all(&mut source, &mut failing, &mut scratch))?)?
print(failing.data.to_text()?, source.at)?
mut again = fake("abcdefghijkl", []?)?
mut stuck = fake("", [2, 0]?)?
print(str.repr(std.io.copy_all(&mut again, &mut stuck, &mut scratch))?)?
mut lying = fake("abcdefghijkl", [-3]?)?
mut sink = fake("", []?)?
print(str.repr(std.io.copy_all(&mut lying, &mut sink, &mut scratch))?)?
mut none = std.bytes.empty()?
print(str.repr(std.io.copy_all(&mut lying, &mut sink, &mut none))?)?
"#,
        r#"
Result[i64, std.io.CopyError].Err(std.io.CopyError.Read(8, std.io.StreamError.Io(IoError.Other)))
abcdefgh
Result[i64, std.io.CopyError].Err(std.io.CopyError.Write(7, std.io.StreamError.Io(IoError.Other)))
abcdefg 8
Result[i64, std.io.CopyError].Err(std.io.CopyError.Write(2, std.io.StreamError.WriteZero))
Result[i64, std.io.CopyError].Err(std.io.CopyError.Read(0, std.io.StreamError.InvalidProgress))
Result[i64, std.io.CopyError].Err(std.io.CopyError.Read(0, std.io.StreamError.OutOfRange))
"#,
    );
}

#[test]
fn buffered_reader_serves_small_reads_from_one_bounded_fill() {
    native(
        r#"
mut reader = std.io.buffered_reader(fake("abcdefghijklmnop", []?)?, 8)?
mut buffer = blank(3)?
mut seen = std.bytes.empty()?
for n in range(4):
    count = reader.read(&mut buffer, 0, 3)?
    seen.extend_range(&buffer, 0, count)?
    print(count, reader.buffered(), reader.inner.calls)?
print(seen.to_text()?, reader.capacity())?
mut large = blank(12)?
print(reader.read(&mut large, 0, 12)?, reader.inner.calls, reader.buffered())?
print(reader.read(&mut large, 0, 12)?, reader.inner.calls, reader.buffered())?
print(large.to_text()?)?
print(reader.read(&mut large, 0, 12)?, reader.read(&mut large, 4, 4)?)?
print(str.repr(reader.read(&mut large, 4, 13))?)?
print(str.repr(std.io.buffered_reader(fake("", []?)?, 0))?)?
"#,
        r#"
3 5 1
3 2 1
2 0 1
3 5 2
abcdefghijk 8
5 2 0
0 3 0
lmnop.......
0 0
Result[i64, std.io.StreamError].Err(std.io.StreamError.OutOfRange)
Result[std.io.BufReader[Fake], std.io.StreamError].Err(std.io.StreamError.OutOfRange)
"#,
    );
}

#[test]
fn buffered_reader_passes_short_reads_interruptions_and_failures_through() {
    native(
        r#"
mut reader = std.io.buffered_reader(fake("abcdefgh", [2, -1, -2, -3, 3]?)?, 4)?
mut buffer = blank(3)?
print(str.repr(reader.read(&mut buffer, 0, 3))?, reader.buffered())?
print(str.repr(reader.read(&mut buffer, 0, 3))?, reader.buffered())?
print(str.repr(reader.read(&mut buffer, 0, 3))?, reader.buffered())?
print(str.repr(reader.read(&mut buffer, 0, 3))?, reader.buffered())?
print(str.repr(reader.read(&mut buffer, 0, 3))?, reader.buffered())?
print(buffer.to_text()?)?
mut exact = std.io.buffered_reader(fake("abcdefgh", [1, -1, 1]?)?, 4)?
mut whole = blank(8)?
print(str.repr(std.io.read_exact(&mut exact, &mut whole, 0, 8))?)?
print(whole.to_text()?)?
"#,
        r#"
Result[i64, std.io.StreamError].Ok(2) 0
Result[i64, std.io.StreamError].Err(std.io.StreamError.Interrupted) 0
Result[i64, std.io.StreamError].Err(std.io.StreamError.Io(IoError.Other)) 0
Result[i64, std.io.StreamError].Err(std.io.StreamError.InvalidProgress) 0
Result[i64, std.io.StreamError].Ok(3) 0
cde
Result[(), std.io.Partial].Ok(())
abcdefgh
"#,
    );
}

#[test]
fn read_until_stops_at_the_delimiter_the_limit_or_end_of_input() {
    native(
        r#"
mut reader = std.io.buffered_reader(fake("one\ntwo-is-long\n\nend", [2, -1]?)?, 4)?
mut line = std.bytes.empty()?
for n in range(7):
    line.clear()
    count = reader.read_until(10u8, &mut line, 6)?
    print(count, str.repr(line.to_text()?)?)?
print(reader.capacity())?
print(str.repr(reader.read_until(10u8, &mut line, 0))?)?
mut failing = std.io.buffered_reader(fake("abcdef", [3, -2]?)?, 4)?
print(str.repr(failing.read_until(10u8, &mut line, 100))?)?
print(line.to_text()?)?
"#,
        r#"
4 "one\n"
6 "two-is"
6 "-long\n"
1 "\n"
3 "end"
0 ""
0 ""
4
Result[i64, std.io.StreamError].Err(std.io.StreamError.OutOfRange)
Result[i64, std.io.StreamError].Err(std.io.StreamError.Io(IoError.Other))
abc
"#,
    );
}

#[test]
fn buffered_writer_holds_at_most_its_capacity() {
    native(
        r#"
mut writer = std.io.buffered_writer(fake("", []?)?, 8)?
source = std.bytes.from_text("abcdefghijklmnopqrstuvwxyz")?
print(writer.write(&source, 0, 3)?, writer.pending(), writer.inner.calls)?
print(writer.write(&source, 3, 8)?, writer.pending(), writer.inner.calls)?
print(writer.write(&source, 8, 10)?, writer.pending(), writer.inner.calls)?
print(writer.inner.data.to_text()?)?
print(writer.write(&source, 10, 22)?, writer.pending(), writer.inner.calls)?
print(writer.inner.data.to_text()?)?
print(writer.write(&source, 22, 26)?, writer.pending(), writer.inner.flushes)?
print(str.repr(writer.flush())?, writer.pending(), writer.inner.flushes)?
print(writer.inner.data.to_text()?, writer.capacity())?
print(writer.write(&source, 5, 5)?, str.repr(writer.write(&source, 20, 27))?)?
print(str.repr(std.io.buffered_writer(fake("", []?)?, 0))?)?
"#,
        r#"
3 3 0
5 8 0
2 2 1
abcdefgh
12 0 3
abcdefghijklmnopqrstuv
4 4 0
Result[(), std.io.StreamError].Ok(()) 0 1
abcdefghijklmnopqrstuvwxyz 8
0 Result[i64, std.io.StreamError].Err(std.io.StreamError.OutOfRange)
Result[std.io.BufWriter[Fake], std.io.StreamError].Err(std.io.StreamError.OutOfRange)
"#,
    );
}

#[test]
fn buffered_writer_keeps_undelivered_bytes_after_a_failed_drain() {
    native(
        r#"
mut writer = std.io.buffered_writer(fake("", [2, -1, 1, -2, 0, -3]?)?, 8)?
source = std.bytes.from_text("abcdefgh")?
print(writer.write(&source, 0, 6)?, writer.pending())?
print(str.repr(writer.flush())?, writer.pending(), writer.inner.flushes)?
print(str.repr(writer.write(&source, 6, 8))?, writer.pending())?
print(str.repr(writer.flush())?, writer.pending())?
print(str.repr(writer.write(&source, 0, 8))?, writer.pending())?
print(str.repr(writer.flush())?, writer.pending(), writer.inner.flushes)?
print(writer.inner.data.to_text()?)?
mut direct = std.io.buffered_writer(fake("", [3]?)?, 4)?
print(str.repr(direct.write(&source, 0, 8))?, direct.pending())?
print(str.repr(std.io.write_all(&mut direct, &source, 3, 8))?, direct.inner.data.to_text()?)?
"#,
        r#"
6 6
Result[(), std.io.StreamError].Err(std.io.StreamError.Io(IoError.Other)) 3 0
Result[i64, std.io.StreamError].Ok(2) 5
Result[(), std.io.StreamError].Err(std.io.StreamError.WriteZero) 5
Result[i64, std.io.StreamError].Err(std.io.StreamError.InvalidProgress) 5
Result[(), std.io.StreamError].Ok(()) 0 1
abcdefgh
Result[i64, std.io.StreamError].Ok(3) 0
Result[(), std.io.Partial].Ok(()) abcdefgh
"#,
    );
}

#[test]
fn close_buffered_always_closes_and_separates_flush_from_close_errors() {
    native(
        r#"
source = std.bytes.from_text("abcd")?
mut clean = std.io.buffered_writer(fake("", []?)?, 8)?
clean.write(&source, 0, 4)?
print(str.repr(std.io.close_buffered(&mut clean))?)?
print(clean.inner.data.to_text()?, clean.inner.flushes, clean.inner.closes)?
mut unflushed = std.io.buffered_writer(fake("", []?)?, 8)?
unflushed.inner.fail_flush = True
unflushed.write(&source, 0, 4)?
print(str.repr(std.io.close_buffered(&mut unflushed))?)?
print(unflushed.inner.data.to_text()?, unflushed.pending(), unflushed.inner.closes)?
mut undelivered = std.io.buffered_writer(fake("", [1, -2]?)?, 8)?
undelivered.write(&source, 0, 4)?
print(str.repr(std.io.close_buffered(&mut undelivered))?)?
print(undelivered.inner.data.to_text()?, undelivered.pending(), undelivered.inner.flushes, undelivered.inner.closes)?
mut unclosed = std.io.buffered_writer(fake("", []?)?, 8)?
unclosed.inner.fail_close = True
unclosed.write(&source, 0, 4)?
print(str.repr(std.io.close_buffered(&mut unclosed))?)?
print(unclosed.inner.data.to_text()?, unclosed.pending())?
mut both = std.io.buffered_writer(fake("", [0]?)?, 8)?
both.inner.fail_close = True
both.write(&source, 0, 4)?
print(str.repr(std.io.close_buffered(&mut both))?)?
print(both.pending(), both.inner.closes)?
"#,
        r#"
Result[(), std.io.CloseError].Ok(())
abcd 1 1
Result[(), std.io.CloseError].Err(std.io.CloseError.Flush(std.io.StreamError.Io(IoError.NotWritable)))
abcd 0 1
Result[(), std.io.CloseError].Err(std.io.CloseError.Flush(std.io.StreamError.Io(IoError.Other)))
a 3 0 1
Result[(), std.io.CloseError].Err(std.io.CloseError.Close(std.io.StreamError.Io(IoError.Other)))
abcd 0
Result[(), std.io.CloseError].Err(std.io.CloseError.FlushAndClose(std.io.StreamError.WriteZero, std.io.StreamError.Io(IoError.Other)))
4 1
"#,
    );
}

#[test]
fn cursor_reads_overwrites_extends_and_seeks() {
    native(
        r#"
mut stream = std.io.cursor(std.bytes.from_text("abcdef")?)
mut buffer = blank(4)?
print(stream.read(&mut buffer, 0, 4)?, stream.position(), buffer.to_text()?)?
print(stream.read(&mut buffer, 0, 4)?, stream.position(), buffer.to_text()?)?
print(stream.read(&mut buffer, 0, 4)?, stream.position())?
print(stream.seek(SeekFrom.Start(4))?, stream.write(&buffer, 0, 4)?, stream.data.to_text()?)?
print(stream.seek(SeekFrom.End(-3))?, stream.seek(SeekFrom.Current(-2))?)?
print(stream.write(&buffer, 2, 3)?, stream.position(), stream.data.to_text()?)?
print(stream.seek(SeekFrom.End(2))?, stream.read(&mut buffer, 0, 4)?)?
print(stream.write(&buffer, 0, 1)?, stream.data.len(), stream.data.get(9)?, stream.data.get(10)?)?
print(str.repr(stream.seek(SeekFrom.Start(-1)))?, stream.position())?
print(str.repr(stream.seek(SeekFrom.Current(-12)))?, stream.position())?
print(str.repr(stream.seek(SeekFrom.Current(i64.MAX)))?, stream.position())?
print(str.repr(stream.read(&mut buffer, 3, 5))?, str.repr(stream.write(&buffer, 3, 5))?)?
print(str.repr(stream.flush())?)?
"#,
        r#"
4 4 abcd
2 6 efcd
0 6
4 4 abcdefcd
5 3
1 4 abccefcd
10 0
1 11 0 101
Result[i64, std.io.StreamError].Err(std.io.StreamError.OutOfRange) 11
Result[i64, std.io.StreamError].Err(std.io.StreamError.OutOfRange) 11
Result[i64, std.io.StreamError].Err(std.io.StreamError.OutOfRange) 11
Result[i64, std.io.StreamError].Err(std.io.StreamError.OutOfRange) Result[i64, std.io.StreamError].Err(std.io.StreamError.OutOfRange)
Result[(), std.io.StreamError].Ok(())
"#,
    );
}

#[test]
fn buffered_seeks_account_for_held_bytes() {
    native(
        r#"
mut reader = std.io.buffered_reader(std.io.cursor(std.bytes.from_text("0123456789")?), 8)?
mut buffer = blank(2)?
print(reader.read(&mut buffer, 0, 2)?, reader.buffered(), reader.inner.position())?
print(std.io.seek_reader(&mut reader, SeekFrom.Current(0))?, reader.buffered(), reader.inner.position())?
print(reader.read(&mut buffer, 0, 2)?, buffer.to_text()?)?
print(std.io.seek_reader(&mut reader, SeekFrom.Current(3))?, reader.read(&mut buffer, 0, 2)?, buffer.to_text()?)?
print(std.io.seek_reader(&mut reader, SeekFrom.Start(1))?, reader.read(&mut buffer, 0, 2)?, buffer.to_text()?)?
print(std.io.seek_reader(&mut reader, SeekFrom.End(-1))?, reader.read(&mut buffer, 0, 2)?, buffer.to_text()?)?
print(str.repr(std.io.seek_reader(&mut reader, SeekFrom.Start(-1)))?)?
mut writer = std.io.buffered_writer(std.io.cursor(std.bytes.empty()?), 8)?
source = std.bytes.from_text("abcdef")?
print(writer.write(&source, 0, 6)?, writer.inner.data.len())?
print(std.io.seek_writer(&mut writer, SeekFrom.Start(1))?, writer.pending(), writer.inner.data.to_text()?)?
print(writer.write(&source, 4, 6)?, str.repr(writer.flush())?, writer.inner.data.to_text()?)?
"#,
        r#"
2 6 8
2 0 2
2 23
7 2 78
1 2 12
9 1 92
Result[i64, std.io.StreamError].Err(std.io.StreamError.OutOfRange)
6 0
1 0 abcdef
2 Result[(), std.io.StreamError].Ok(()) aefdef
"#,
    );
}

#[test]
fn file_stream_reads_writes_and_seeks_bytes() {
    native(
        r#"
mut stream = std.io.open_file("data.bin", "w+")?
source = std.bytes.from_list(&[0u8, 255u8, 10u8, 13u8, 65u8, 66u8]?)?
print(str.repr(std.io.write_all(&mut stream, &source, 0, 6))?)?
print(str.repr(stream.flush())?, str.repr(stream.sync())?)?
print(stream.seek(SeekFrom.Current(0))?, stream.seek(SeekFrom.Start(1))?)?
mut buffer = blank(8)?
print(stream.read(&mut buffer, 0, 8)?, buffer.to_list()?)?
print(stream.read(&mut buffer, 0, 8)?, stream.read(&mut buffer, 2, 2)?)?
print(stream.seek(SeekFrom.End(-2))?, stream.read(&mut buffer, 0, 1)?, buffer.get(0)?)?
print(stream.seek(SeekFrom.Current(-3))?, stream.seek(SeekFrom.End(4))?)?
print(str.repr(stream.seek(SeekFrom.Start(-1)))?, str.repr(stream.seek(SeekFrom.Current(-100)))?)?
print(str.repr(stream.read(&mut buffer, 4, 9))?, str.repr(stream.write(&source, 4, 7))?)?
print(str.repr(stream.close())?, str.repr(stream.close())?, stream.file.closed)?
print(str.repr(stream.read(&mut buffer, 0, 1))?)?
print(str.repr(stream.write(&source, 0, 1))?)?
print(str.repr(stream.seek(SeekFrom.Start(0)))?)?
print(str.repr(stream.flush())?)?
"#,
        r#"
Result[(), std.io.Partial].Ok(())
Result[(), std.io.StreamError].Ok(()) Result[(), std.io.StreamError].Ok(())
6 1
5 [255, 10, 13, 65, 66, 46, 46, 46]
0 0
4 1 65
2 10
Result[i64, std.io.StreamError].Err(std.io.StreamError.OutOfRange) Result[i64, std.io.StreamError].Err(std.io.StreamError.Io(IoError.System(22)))
Result[i64, std.io.StreamError].Err(std.io.StreamError.OutOfRange) Result[i64, std.io.StreamError].Err(std.io.StreamError.OutOfRange)
Result[(), std.io.StreamError].Ok(()) Result[(), std.io.StreamError].Ok(()) True
Result[i64, std.io.StreamError].Err(std.io.StreamError.Io(IoError.Closed))
Result[i64, std.io.StreamError].Err(std.io.StreamError.Io(IoError.Closed))
Result[i64, std.io.StreamError].Err(std.io.StreamError.Io(IoError.Closed))
Result[(), std.io.StreamError].Err(std.io.StreamError.Io(IoError.Closed))
"#,
    );
}

#[test]
fn file_streams_enforce_the_open_mode() {
    native(
        r#"
mut created = std.io.open_file("mode.bin", "w")?
source = std.bytes.from_text("abc")?
mut buffer = blank(3)?
print(created.write(&source, 0, 3)?, str.repr(created.read(&mut buffer, 0, 3))?)?
created.close()?
mut reopened = std.io.from_file(open("mode.bin", "r")?)
print(reopened.read(&mut buffer, 0, 3)?, buffer.to_text()?, str.repr(reopened.write(&source, 0, 3))?)?
match std.io.open_file("missing/none.bin", "r"):
    case Ok(_):
        print("opened")?
    case Err(cause):
        print(str.repr(cause)?)?
match std.io.open_file("mode.bin", "sideways"):
    case Ok(_):
        print("opened")?
    case Err(cause):
        print(str.repr(cause)?)?
"#,
        r#"
3 Result[i64, std.io.StreamError].Err(std.io.StreamError.Io(IoError.NotReadable))
3 abc Result[i64, std.io.StreamError].Err(std.io.StreamError.Io(IoError.NotWritable))
std.io.StreamError.Io(IoError.System(2))
std.io.StreamError.Io(IoError.InvalidMode)
"#,
    );
}

#[test]
fn buffered_file_copy_round_trips() {
    native(
        r#"
mut text = std.bytes.empty()?
for n in range(1000):
    text.push(u8.wrapping_from(n))?
mut writer = std.io.buffered_writer(std.io.open_file("copy.bin", "w+")?, 64)?
mut source = std.io.cursor(text)
mut scratch = blank(48)?
print(std.io.copy_all(&mut source, &mut writer, &mut scratch).unwrap(), writer.pending())?
print(std.io.seek_writer(&mut writer, SeekFrom.Start(0))?, writer.pending())?
mut reader = std.io.buffered_reader(std.io.from_file(replace(writer.inner.file, open("copy.bin", "r")?)), 32)?
mut back = std.bytes.empty()?
print(std.io.read_up_to(&mut reader, &mut back, 5000).unwrap(), back == source.data)?
print(str.repr(reader.inner.close())?, str.repr(std.io.close_buffered(&mut writer))?)?
"#,
        r#"
1000 40
0 0
1000 True
Result[(), std.io.StreamError].Ok(()) Result[(), std.io.CloseError].Ok(())
"#,
    );
}

#[test]
fn standard_stream_adapters_move_raw_bytes() {
    let input = b"first\nsecond line\n\xff\x00tail";
    let run = run(
        &program(
            r#"
mut input = std.io.buffered_reader(std.io.stdin(), 4)?
mut output = std.io.buffered_writer(std.io.stdout(), 4)?
mut errors = std.io.stderr()
mut line = std.bytes.empty()?
mut lines = 0
while input.read_until(10u8, &mut line, 64)? > 0:
    lines += 1
    std.io.write_all(&mut output, &line, 0, line.len()).unwrap()
    line.clear()
output.flush()?
note = std.bytes.from_text("done\n")?
std.io.write_all(&mut errors, &note, 0, note.len()).unwrap()
errors.flush()?
print(lines, input.read_until(10u8, &mut line, 64)?)?
"#,
        ),
        input,
    );
    // Buffered output is flushed before `print` writes the summary line.
    let mut expected = input.to_vec();
    expected.extend_from_slice(b"3 0");
    assert_eq!(
        run.stdout.as_bytes(),
        String::from_utf8_lossy(&expected).as_bytes()
    );
    assert_eq!(run.stderr, "done\n");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn buffered_transfers_within_capacity_do_not_allocate() {
    native(
        r#"
mut reader = std.io.buffered_reader(std.io.cursor(std.bytes.from_text("one\ntwo\nthree\n")?), 4)?
mut writer = std.io.buffered_writer(std.io.cursor(std.bytes.with_capacity(64)?), 4)?
mut line = std.bytes.with_capacity(16)?
mut scratch = blank(8)?
print("__test_fail_allocations_after_0__")?
print("__test_begin_no_allocations__")?
first = reader.read_until(10u8, &mut line, 16)
exact = std.io.read_exact(&mut reader, &mut scratch, 0, 4)
written = std.io.write_all(&mut writer, &line, 0, line.len())
copied = std.io.copy_all(&mut reader, &mut writer, &mut scratch)
flushed = writer.flush()
sought = std.io.seek_writer(&mut writer, SeekFrom.Start(0))
print("__test_restore_allocations__")?
print("__test_end_no_allocations__")?
print(str.repr(first)?, str.repr(exact)?, str.repr(written)?)?
print(str.repr(copied)?, str.repr(flushed)?, str.repr(sought)?)?
print(str.repr(writer.inner.data.to_text()?)?)?
"#,
        r#"
Result[i64, std.io.StreamError].Ok(4) Result[(), std.io.Partial].Ok(()) Result[(), std.io.Partial].Ok(())
Result[i64, std.io.CopyError].Ok(6) Result[(), std.io.StreamError].Ok(()) Result[i64, std.io.StreamError].Ok(0)
"one\nthree\n"
"#,
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_is_reported_with_the_progress_made() {
    native(
        r#"
mut reader = fake("abcdefgh", [3]?)?
mut out = std.bytes.empty()?
mut stream = std.io.cursor(std.bytes.empty()?)
source = std.bytes.from_text("abcd")?
mut held = std.io.buffered_reader(fake("abcdefgh", []?)?, 4)?
mut line = std.bytes.empty()?
print("__test_fail_allocations_after_0__")?
a = std.io.buffered_reader(std.io.stdin(), 16)
b = std.io.buffered_writer(std.io.stdout(), 16)
c = std.io.read_up_to(&mut reader, &mut out, 8)
d = stream.write(&source, 0, 4)
e = held.read_until(10u8, &mut line, 8)
print("__test_restore_allocations__")?
print(str.repr(a)?)?
print(str.repr(b)?)?
print(str.repr(c)?, out.len(), reader.calls)?
print(str.repr(d)?, stream.data.len(), stream.position())?
print(str.repr(e)?, line.len(), held.buffered())?
print(str.repr(std.io.read_up_to(&mut reader, &mut out, 8))?, out.to_text()?)?
"#,
        r#"
Result[std.io.BufReader[std.io.Stdin], std.io.StreamError].Err(std.io.StreamError.Allocation(AllocError.OutOfMemory))
Result[std.io.BufWriter[std.io.Stdout], std.io.StreamError].Err(std.io.StreamError.Allocation(AllocError.OutOfMemory))
Result[i64, std.io.Partial].Err(std.io.Partial(completed=0, cause=std.io.StreamError.Allocation(AllocError.OutOfMemory))) 0 0
Result[i64, std.io.StreamError].Err(std.io.StreamError.Allocation(AllocError.OutOfMemory)) 0 0
Result[i64, std.io.StreamError].Err(std.io.StreamError.Allocation(AllocError.OutOfMemory)) 0 4
Result[i64, std.io.Partial].Ok(8) abcdefgh
"#,
    );
}

#[test]
fn reading_needs_an_exclusive_buffer() {
    let error = rejected(
        r#"
mut reader = fake("abc", []?)?
buffer = blank(3)?
std.io.read_exact(&mut reader, &buffer, 0, 3).unwrap()
"#,
    );
    assert!(
        error.contains("mutable reference arguments require explicit &mut"),
        "{error}"
    );
}

#[test]
fn helpers_reject_classes_without_the_stream_methods() {
    for (body, expected) in [
        (
            r#"
mut buffer = blank(3)?
mut other = blank(3)?
std.io.read_exact(&mut other, &mut buffer, 0, 3).unwrap()
"#,
            "std.bytes.Bytes does not satisfy std.io.Reader: missing method `read`",
        ),
        (
            r#"
mut reader = std.io.buffered_reader(std.io.stdin(), 4)?
source = blank(3)?
std.io.write_all(&mut reader, &source, 0, 3).unwrap()
"#,
            "std.io.BufReader[std.io.Stdin] does not satisfy std.io.Writer: missing method `write`",
        ),
        (
            r#"
mut writer = std.io.buffered_writer(std.io.stdout(), 4)?
print(std.io.seek_writer(&mut writer, SeekFrom.Start(0))?)?
"#,
            "std.io.Stdout does not satisfy std.io.Seeker: missing method `seek`",
        ),
    ] {
        let error = rejected(body);
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn a_stream_cannot_read_into_its_own_storage() {
    let error = rejected(
        r#"
mut stream = std.io.cursor(blank(3)?)
print(stream.read(&mut stream.data, 0, 3)?)?
"#,
    );
    assert!(error.contains("conflicting borrow"), "{error}");
}

#[test]
fn buffer_state_is_private_and_the_inner_stream_stays_owned() {
    let error = rejected(
        r#"
mut reader = std.io.buffered_reader(fake("abc", []?)?, 4)?
print(reader._start)?
"#,
    );
    assert!(
        error.contains("`std.io.BufReader._start` is private"),
        "{error}"
    );
    let error = rejected(
        r#"
mut reader = std.io.buffered_reader(fake("abc", []?)?, 4)?
taken = reader.inner
print(reader.buffered())?
"#,
    );
    assert!(error.contains("cannot move out of a field"), "{error}");
}
