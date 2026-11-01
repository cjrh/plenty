//! Read-only byte operations and stream writes take a `&[u8]` view of a
//! `bytes` literal, a `Bytes` buffer or a `list[u8]` in place (issue #156,
//! backlog B90), and protocol requirements may declare their own type
//! parameters.
mod support;
use support::{check_error as rejected, check_file_error as file_rejected};

/// Programs that import `std` need a file-based compilation.
fn file_output(source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let output = std::process::Command::new(executable)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn buffer_operations_read_literals_and_buffers_alike() {
    let printed = file_output(
        r#"
import std.bytes

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:
    mut head = std.bytes.from_bytes(b"GET / HTTP/1.1\r\nHost: a\r\n\r\n")?
    crlf = std.bytes.from_text("\r\n")?
    print(head.find_bytes(b"\r\n", 0, head.len())?, head.find_bytes(&crlf, 15, head.len())?)?
    print(head.find_bytes(b"", 3, 9)?, head.find_bytes(b"\r\n\r\n", 0, head.len())?)?
    print(head.find_in(b" :", 0, head.len())?, head.find_not_in(b"GET", 0, head.len())?)?
    print(head.find_in(b"", 0, head.len())?)?
    print(head.matches(0, b"GET")?, head.matches(0, &b"xGET"[1:4])?, head.matches(25, b"\r\n\r\n")?)?
    literal = b"\x00\xff"
    mut out = std.bytes.empty()?
    out.extend(&literal)?
    out.extend(b"abc")?
    out.extend(&b"abc"[1:3])?
    out.extend(&crlf)?
    out.overwrite(2, &b"XYZ"[:2])?
    print(out)?
    Ok(())
"#,
    );
    assert_eq!(
        printed,
        "Option[i64].Some(14) Option[i64].Some(23)\n\
         Option[i64].Some(3) Option[i64].Some(23)\n\
         Option[i64].Some(3) Option[i64].Some(3)\n\
         Option[i64].Nothing\n\
         True True False\n\
         b\"\\x00\\xffXYcbc\\r\\n\"\n"
    );
}

/// A view carries its own range, and its bounds clamp like `xs[a:b]`, so
/// reading through one has no range error; offsets into the receiver are
/// still checked.
#[test]
fn views_clamp_and_offsets_stay_checked() {
    let printed = file_output(
        r#"
import std.bytes

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:
    mut out = std.bytes.from_text("abcd")?
    out.extend(&b"xy"[1:9])?
    out.extend(&b"xy"[-1:1])?
    print(out, out.matches(0, &b"ab"[:3])?)?
    print(str.repr(out.overwrite(4, b"xy"))?)?
    print(str.repr(out.matches(6, b"ab"))?)?
    print(out.matches(3, b"dy")?, out.len())?
    Ok(())
"#,
    );
    assert_eq!(
        printed,
        "b\"abcdy\" True\n\
         Result[(), std.bytes.BytesError].Err(std.bytes.BytesError.OutOfRange)\n\
         Result[bool, std.bytes.BytesError].Err(std.bytes.BytesError.OutOfRange)\n\
         True 5\n"
    );
}

#[test]
fn stream_writes_take_literals() {
    let printed = file_output(
        r#"
import std.bytes
import std.compress
import std.io
from std.compress import Format

def main() -> Result[(), AllocError | IoError | std.io.StreamError | std.io.Partial | std.bytes.BytesError | std.compress.CompressError]:
    mut out = std.io.stdout()
    out.write(b"direct\n")?
    std.io.write_all(&mut out, &b"[all]\n"[1:6])?
    mut buffered = std.io.buffered_writer(std.io.stdout(), 4)?
    buffered.write(b"ab")?
    buffered.write(b"cdefgh\n")?
    buffered.flush()?
    mut memory = std.io.cursor(std.bytes.empty()?)
    std.io.write_all(&mut memory, b"memory")?
    print(memory.data.to_text()?)?
    mut file = std.io.open_file("sources.bin", "w+")?
    std.io.write_all(&mut file, b"\x00\xfffile")?
    file.seek(std.io.SeekFrom.Start(0))?
    mut back = std.bytes.with_capacity(6)?
    back.resize(6, 1u8)?
    std.io.read_exact(&mut file, &mut back, 0, 6)?
    print(back)?
    mut packer = std.compress.compressor(Format.Gzip, std.io.cursor(std.bytes.empty()?), 6)?
    std.io.write_all(&mut packer, b"compressed literal")?
    packer.finish()?
    unpacked = std.compress.decompress(Format.Gzip, &packer.inner.data, 64)?
    print(unpacked.to_text()?)?
    print(out.write(&b"ab"[1:3])?)?
    Ok(())
"#,
    );
    assert_eq!(
        printed,
        "direct\nall]\nabcdefgh\nmemory\nb\"\\x00\\xfffile\"\n\
         compressed literal\nb1\n"
    );
}

#[test]
fn writers_declare_a_write_taking_a_byte_view() {
    let printed = file_output(
        r#"
import std.bytes
import std.io
from std.bytes import Bytes
from std.io import StreamError

pub class Upper:
    pub data: Bytes

    pub def write(self: &mut Upper, source: &[u8]) -> Result[i64, StreamError]:
        for index in range(len(source)):
            mut byte = source[index]
            if byte >= 97u8 and byte <= 122u8:
                byte -= 32u8
            match self.data.push(byte):
                case Ok(_):
                    pass
                case Err(_):
                    return Err(StreamError.OutOfRange)
        Ok(len(source))

    pub def flush(self: &mut Upper) -> Result[(), StreamError]:
        Ok(())

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | std.io.Partial | std.io.StreamError]:
    mut upper = Upper(std.bytes.empty()?)
    std.io.write_all(&mut upper, b"literal ")?
    word = std.bytes.from_text("buffer")?
    std.io.write_all(&mut upper, &word[:6])?
    mut buffered = std.io.buffered_writer(upper, 4)?
    buffered.write(b"!")?
    buffered.flush()?
    print(buffered.inner.data.to_text()?)?
    Ok(())
"#,
    );
    assert_eq!(printed, "LITERAL BUFFER!\n");
}

#[test]
fn a_writer_with_a_monomorphic_write_is_rejected() {
    let message = file_rejected(
        r#"
import std.bytes
import std.io
from std.bytes import Bytes
from std.io import StreamError

pub class Old:
    pub data: Bytes

    pub def write(self: &mut Old, source: &Bytes, start: i64, stop: i64) -> Result[i64, StreamError]:
        Ok(stop - start)

    pub def flush(self: &mut Old) -> Result[(), StreamError]:
        Ok(())

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | StreamError]:
    mut old = Old(std.bytes.empty()?)
    std.io.write_all(&mut old, b"x")?
    Ok(())
"#,
    );
    assert!(
        message.contains("Old does not satisfy std.io.Writer: signature of `write` does not match"),
        "{message}"
    );
}

#[test]
fn only_byte_views_are_read_in_place() {
    let message = file_rejected(
        r#"
import std.bytes

pub class Sized:
    pub count: i64

    pub def len(self) -> i64:
        self.count

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:
    data = std.bytes.from_text("abc")?
    print(data.find_bytes(&Sized(1), 0, 3)?)?
    Ok(())
"#,
    );
    assert!(
        message.contains("this parameter takes a slice view"),
        "{message}"
    );
}

#[test]
fn bytes_satisfies_protocols_through_len() {
    let output = support::run(
        r#"
protocol Measured:
    def len(self) -> i64:
        pass

def measure[M: Measured](value: &M) -> i64:
    value.len()

def main() -> Result[(), AllocError | IoError]:
    literal = b"four"
    print(literal.len(), measure(&literal), measure(b""))?
    Ok(())
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "4 4 0\n");
    for (protocol, expected) in [
        (
            "protocol P:\n    def size(self) -> i64:\n        pass\n",
            "bytes does not satisfy P: missing method `size`",
        ),
        (
            "protocol P:\n    def len(self: &mut P) -> i64:\n        pass\n",
            "bytes does not satisfy P: signature of `len` does not match",
        ),
        (
            "protocol P:\n    def len[T](self) -> i64:\n        pass\n",
            "bytes does not satisfy P: signature of `len` does not match",
        ),
    ] {
        let message = rejected(&format!(
            "{protocol}def use[T: P](value: &T) -> ():\n    pass\nuse(b\"x\")\n"
        ));
        assert!(message.contains(expected), "{protocol}: {message}");
    }
    let message = rejected("value = b\"ab\".len(1)\n");
    assert!(message.contains("len takes no arguments"), "{message}");
}

const SINK: &str = "protocol Sized:
    def size(self) -> i64:
        pass
protocol Other:
    def size(self) -> i64:
        pass
protocol Sink:
    def put[S: Sized](self: &mut Sink, item: &S) -> i64:
        pass
class Item:
    width: i64
    def size(self) -> i64:
        self.width
def feed[K: Sink](sink: &mut K) -> i64:
    sink.put(&Item(2))
";

#[test]
fn requirements_take_protocol_constrained_type_parameters() {
    let output = support::run(&format!(
        "{SINK}class Total:
    sum: i64
    def put[Thing: Sized](self: &mut Total, item: &Thing) -> i64:
        self.sum += item.size()
        self.sum
class Twice[T]:
    inner: T
    def put[S: Sized](self: &mut Twice[T], item: &S) -> i64:
        2 * item.size()
def main() -> Result[(), AllocError | IoError]:
    mut total = Total(1)
    feed(&mut total)
    mut twice = Twice(0u8)
    print(feed(&mut total), feed(&mut twice))?
    Ok(())
"
    ));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "5 4\n");
}

#[test]
fn implementations_must_declare_the_requirement_parameters() {
    let mismatch = "signature of `put` does not match";
    for (method, expected) in [
        (
            "    def put(self: &mut Bad, item: &Item) -> i64:\n        1\n",
            mismatch,
        ),
        (
            "    def put[S](self: &mut Bad, item: &S) -> i64:\n        1\n",
            mismatch,
        ),
        (
            "    def put[S: Other](self: &mut Bad, item: &S) -> i64:\n        1\n",
            mismatch,
        ),
        (
            "    def put[S: Sized, U](self: &mut Bad, item: &S) -> i64:\n        1\n",
            mismatch,
        ),
        (
            "    def put[S: Sized](self: &mut Bad, item: &Item) -> i64:\n        1\n",
            mismatch,
        ),
        (
            "    def put[S: Sized](self: &mut Bad, item: S) -> i64:\n        1\n",
            mismatch,
        ),
        (
            "    def other(self) -> i64:\n        1\n",
            "missing method `put`",
        ),
    ] {
        let message = rejected(&format!(
            "{SINK}class Bad:\n    value: i64\n{method}mut bad = Bad(0)\nfeed(&mut bad)\n"
        ));
        assert!(message.contains(expected), "{method}: {message}");
    }
}

#[test]
fn requirement_parameters_are_validated_at_the_protocol() {
    for (protocol, expected) in [
        (
            "protocol P[T]:\n    def put[T](self: &mut P[T], item: &T) -> ():\n        pass\n",
            "a requirement parameter cannot shadow its protocol or the protocol's parameters",
        ),
        (
            "protocol P:\n    def put[P](self: &mut P, item: &P) -> ():\n        pass\n",
            "a requirement parameter cannot shadow its protocol or the protocol's parameters",
        ),
        (
            "protocol P:\n    def put[T: IntType](self: &mut P, item: T) -> ():\n        pass\n",
            "requirement parameters take only protocol constraints",
        ),
        (
            "class C:\n    value: i64\nprotocol P:\n    def put[T: C](self: &mut P, item: &T) -> ():\n        pass\n",
            "requirement parameters take only protocol constraints",
        ),
        (
            "protocol P:\n    def put[T](self: &mut P, item: &Missing) -> ():\n        pass\n",
            "Missing",
        ),
    ] {
        let message = rejected(&format!("{protocol}value = 1\n"));
        assert!(message.contains(expected), "{protocol}: {message}");
    }
}

/// The generic paths pass a literal's read-only data, or a buffer's storage,
/// straight to the native operation: nothing is copied or allocated.
#[cfg(feature = "runtime-checks")]
#[test]
fn reading_and_writing_sources_does_not_allocate() {
    let printed = file_output(
        r#"
import std.bytes
import std.io


def tally(source: &[u8]) -> i64:
    mut total = 0
    for byte in source:
        total += i64(*byte)
    total + len(source)

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | std.io.Partial]:
    mut head = std.bytes.from_bytes(b"GET / HTTP/1.1\r\n\r\n")?
    mut out = std.bytes.with_capacity(64)?
    crlf = std.bytes.from_text("\r\n")?
    mut stdout = std.io.stdout()
    mut memory = std.io.cursor(std.bytes.with_capacity(64)?)
    print("__test_begin_no_allocations__")?
    found = head.find_bytes(b"\r\n\r\n", 0, head.len())?
    also = head.find_bytes(&crlf, 0, head.len())?
    spaces = head.find_in(b" ", 0, head.len())?
    same = head.matches(0, b"GET")?
    out.extend(b"abc")?
    out.extend(&crlf[:2])?
    head.overwrite(0, b"PUT")?
    std.io.write_all(&mut stdout, b"written\n")?
    std.io.write_all(&mut memory, b"memory")?
    sums = tally(b"\x01\x02") + tally(&crlf)
    print("__test_end_no_allocations__")?
    print(found, also, spaces, same, out.len(), sums)?
    Ok(())
"#,
    );
    assert_eq!(
        printed,
        "__test_begin_no_allocations__\nwritten\n__test_end_no_allocations__\n\
         Option[i64].Some(14) Option[i64].Some(14) Option[i64].Some(3) True 5 30\n"
    );
}
