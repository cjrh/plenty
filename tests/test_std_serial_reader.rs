//! Typed JSON loads over bounded incremental byte readers.
mod support;

const READER: &str = r#"
import std.json
import std.serial
import std.bytes
import std.io
import std.math
from std.bytes import Bytes
from std.io import Cursor, StreamError
from std.json import JsonError
from std.serial import SerialError, Fault, Step

pub class Chunks:
    cursor: Cursor
    chunk: i64
    interrupt: bool
    fail_at: i64
    largest: i64

    pub def read(self: &mut Chunks, buffer: &mut Bytes, start: i64, stop: i64) -> Result[i64, StreamError]:
        # The codec supplies 0 <= start <= stop <= 4096.
        span = unchecked:
            stop - start
        self.largest = std.math.max(self.largest, span)
        if self.interrupt:
            self.interrupt = False
            return Err(StreamError.Interrupted)
        if self.fail_at >= 0 and self.cursor.position() >= self.fail_at:
            return Err(StreamError.UnexpectedEof)
        end = unchecked:
            start + std.math.min(span, self.chunk)
        self.cursor.read(buffer, start, end)

def chunks(text: &str, size: i64, fail_at: i64 = -1) -> Result[Chunks, AllocError]:
    Ok(Chunks(std.io.cursor(std.bytes.from_text(text)?), size, True, fail_at, 0))
"#;

#[test]
fn reader_crosses_every_token_boundary_and_checks_eof() {
    support::assert_stdout(
        &format!(
            r#"{READER}
def main() -> Result[(), IoError | AllocError | SerialError[JsonError]]:
    options = std.json.codec_options()
    mut source = std.json.codec_reader(chunks("[[\"A\\uD83D\\uDE00é\",null,18446744073709551615]]  ", 1)?)
    values = std.json.load_from[list[(str, Option[bool], u64)], Chunks](&mut source, &options)?
    print(values[0])?
    print(source.inner.cursor.remaining(), source.inner.largest <= 4096)?
    match std.json.load_from[i64, Chunks](&mut source, &options):
        case Ok(_):
            print("unexpected")?
        case Err(error):
            print(error.cause.fault)?
    mut decimals = std.json.codec_reader(chunks("[1.25e+2,-0.5]", 1)?)
    print(std.json.load_from[list[f64], Chunks](&mut decimals, &options)?)?
    Ok(())
"#
        ),
        "(\"A😀é\", Option[bool].Nothing, 18446744073709551615)\n0 True\nstd.json.JsonFault.InvalidState\n[125.0, -0.5]\n",
    );
}

#[test]
fn reader_preserves_typed_paths_and_failed_stream_owner() {
    support::assert_stdout(
        &format!(
            r#"{READER}
def main() -> Result[(), IoError | AllocError]:
    options = std.json.codec_options()
    mut range = std.json.codec_reader(chunks("[1,256]", 1)?)
    match std.json.load_from[list[u8], Chunks](&mut range, &options):
        case Ok(_):
            print("unexpected")?
        case Err(error):
            print(error.cause.fault, error.path.unwrap())?
    print(range.inner.cursor.position() > 0)?
    mut failing = std.json.codec_reader(chunks("[[1,2]]", 1, 4)?)
    match std.json.load_from[list[list[i64]], Chunks](&mut failing, &options):
        case Ok(_):
            print("unexpected")?
        case Err(error):
            print(error.cause.fault, error.path.unwrap(), error.cause.offset)?
    print(failing.inner.cursor.position())?
    mut completion = std.json.codec_reader(chunks("[] ", 1, 3)?)
    match std.json.load_from[list[i64], Chunks](&mut completion, &options):
        case Ok(_):
            print("unexpected")?
        case Err(error):
            print(error.cause.fault, error.path.is_none())?
    Ok(())
"#
        ),
        "std.json.JsonFault.Serialization(std.serial.Fault.OutOfRange) [std.serial.Step.Index(1)]\nTrue\nstd.json.JsonFault.Stream(std.io.StreamError.UnexpectedEof) [std.serial.Step.Index(0)] 4\n4\nstd.json.JsonFault.Stream(std.io.StreamError.UnexpectedEof) True\n",
    );
}

#[test]
fn reader_enforces_document_input_depth_and_progress_contracts() {
    support::assert_stdout(
        &format!(
            r#"{READER}
pub class InvalidReader:
    pub def read(self: &mut InvalidReader, buffer: &mut Bytes, start: i64, stop: i64) -> Result[i64, StreamError]:
        unchecked:
            Ok(stop - start + 1)

def main() -> Result[(), IoError | AllocError]:
    mut options = std.json.codec_options()
    for text in ["[1] [2]", "[1,", "[\"\\uD800\"]"]?:
        mut source = std.json.codec_reader(chunks(&text, 1)?)
        match std.json.load_from[list[i64], Chunks](&mut source, &options):
            case Ok(_):
                print("unexpected")?
            case Err(error):
                print(error.cause.fault)?
    options.limits.max_input = 1
    mut exact = std.json.codec_reader(chunks("1", 1)?)
    print(std.json.load_from[i64, Chunks](&mut exact, &options).unwrap())?
    mut excess = std.json.codec_reader(chunks("1 ", 1)?)
    match std.json.load_from[i64, Chunks](&mut excess, &options):
        case Ok(_):
            print("unexpected")?
        case Err(error):
            print(error.cause.fault)?
    options.limits.max_input = 100
    options.limits.max_depth = 1
    mut nested = std.json.codec_reader(chunks("[[1]]", 1)?)
    match std.json.load_from[list[list[i64]], Chunks](&mut nested, &options):
        case Ok(_):
            print("unexpected")?
        case Err(error):
            print(error.cause.fault)?
    mut invalid = std.json.codec_reader(InvalidReader())
    match std.json.load_from[i64, InvalidReader](&mut invalid, &options):
        case Ok(_):
            print("unexpected")?
        case Err(error):
            print(error.cause.fault)?
    Ok(())
"#
        ),
        "std.json.JsonFault.TrailingData\nstd.json.JsonFault.UnexpectedEnd\nstd.json.JsonFault.InvalidSurrogate\n1\nstd.json.JsonFault.InputTooLarge\nstd.json.JsonFault.DepthExceeded\nstd.json.JsonFault.Stream(std.io.StreamError.InvalidProgress)\n",
    );
}

const RECORD: &str = r#"
class Record:
    value: i64

    def __decode__[E, D: std.serial.Decoder[E]](source: &mut D) -> Result[Record, SerialError[E]]:
        std.serial.wrap(source.start_object())?
        mut value = 0
        while True:
            match std.serial.wrap(source.next_key())?:
                case Some(name):
                    if name == "value":
                        value = std.serial.at(std.serial.decode[i64, E, D](source), Step.Field(name))?
                    else:
                        std.serial.at(std.serial.wrap(source.unknown_key(&name)), Step.Field(name))?
                case Nothing:
                    break
        Ok(Record(value))
"#;

#[test]
fn reader_skips_large_unknown_values_without_a_value_tree() {
    let document = format!(
        "{{\"ignored\":[{}],\"value\":7}}",
        vec!["{\"text\":\"payload\"}"; 700].join(",")
    );
    let literal = format!("{document:?}");
    support::assert_stdout(
        &format!(
            r#"{READER}{RECORD}
def main() -> Result[(), IoError | AllocError | SerialError[JsonError]]:
    mut options = std.json.codec_options()
    options.unknown_fields = std.serial.UnknownFields.Ignore
    mut source = std.json.codec_reader(chunks({literal}, 37)?)
    value = std.json.load_from[Record, Chunks](&mut source, &options)?
    print(value.value, source.inner.cursor.remaining(), source.inner.largest)?
    options.unknown_fields = std.serial.UnknownFields.Reject
    mut rejected = std.json.codec_reader(chunks("{{\"ignored\":[1]}}", 1)?)
    match std.json.load_from[Record, Chunks](&mut rejected, &options):
        case Ok(_):
            print("unexpected")?
        case Err(error):
            print(error.cause.fault, error.path.unwrap())?
    Ok(())
"#
        ),
        "7 0 4096\nstd.json.JsonFault.Serialization(std.serial.Fault.UnknownField(\"ignored\")) [std.serial.Step.Field(\"ignored\")]\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn reader_initialization_failure_retains_unread_owner_and_can_retry() {
    support::assert_stdout(
        &format!(
            r#"{READER}
def main() -> Result[(), IoError | AllocError | SerialError[JsonError]]:
    options = std.json.codec_options()
    mut source = std.json.codec_reader(chunks("17", 1)?)
    print("__test_fail_allocations_after_0__")?
    outcome = std.json.load_from[i64, Chunks](&mut source, &options)
    print("__test_restore_allocations__")?
    match outcome:
        case Ok(_):
            print("unexpected")?
        case Err(error):
            print(error.cause.fault)?
    print(source.inner.cursor.position())?
    print(std.json.load_from[i64, Chunks](&mut source, &options)?)?
    Ok(())
"#
        ),
        "__test_fail_allocations_after_0__\n__test_restore_allocations__\nstd.json.JsonFault.Allocation(AllocError.OutOfMemory)\n0\n17\n",
    );
}
