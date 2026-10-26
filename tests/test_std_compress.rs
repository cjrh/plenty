//! `std.compress`: DEFLATE, zlib and gzip through zlib-rs, checked against
//! fixtures from CPython's zlib and gzip modules and against the system
//! `gzip` tool.
mod support;
use support::{build_file as build, run_file as run};

const HEADER: &str = r#"import std.bytes
import std.compress
import std.encoding
import std.io
from std.bytes import ByteSource, Bytes
from std.compress import Format, CompressError
from std.io import StreamError

def hex(data: &Bytes) -> Result[str, AllocError | IoError | std.encoding.EncodingError]:
    Ok(std.encoding.hex_text(data, 0, data.len())?)

def unhex(text: &str) -> Result[Bytes, AllocError | IoError | std.encoding.EncodingError]:
    Ok(std.encoding.hex_from_text(text)?)

# The decoded text, or the error's name.
def shown(result: Result[Bytes, CompressError]) -> Result[str, AllocError | IoError | std.bytes.BytesError]:
    match result:
        case Ok(data):
            Ok(data.to_text()?)
        case Err(error):
            Ok(str.repr(error)?)

def repeated(text: &str, count: i64) -> Result[Bytes, AllocError | IoError | std.bytes.BytesError]:
    piece = std.bytes.from_text(text)?
    mut out = std.bytes.with_capacity(piece.len() * count)?
    for _ in range(0, count):
        out.extend(&piece)?
    Ok(out)

# Deterministic bytes that do not compress.
def noise(count: i64) -> Result[Bytes, AllocError | IoError | std.bytes.BytesError]:
    mut out = std.bytes.with_capacity(count)?
    mut x = 88172645463325252
    for _ in range(0, count):
        x = x ^ (x << 13)
        x = x ^ (x >> 7)
        x = x ^ (x << 17)
        out.push(u8.wrapping_from(x >> 24))?
    Ok(out)

def trip(format: Format, level: i64, data: &Bytes) -> Result[bool, AllocError | IoError | std.compress.CompressError | std.encoding.EncodingError]:
    packed = std.compress.compress(format, data, level)?
    plain = std.compress.decompress(format, &packed, data.len())?
    Ok(hex(&plain)? == hex(data)?)

# A reader that delivers one byte per call and is interrupted every other call.
pub class Trickle:
    data: Bytes
    at: i64
    pause: bool

    pub def read(self: &mut Trickle, buffer: &mut Bytes, start: i64, stop: i64) -> Result[i64, StreamError]:
        self.pause = not self.pause
        if self.pause:
            return Err(StreamError.Interrupted)
        if start == stop or self.at == self.data.len():
            return Ok(0)
        match self.data.get(self.at):
            case Ok(value):
                match buffer.set(start, value):
                    case Ok(_):
                        pass
                    case Err(_):
                        return Err(StreamError.OutOfRange)
            case Err(_):
                return Err(StreamError.OutOfRange)
        self.at += 1
        Ok(1)

# A writer that accepts one byte per call, is interrupted every other call,
# and fails while `broken`.
pub class Drip:
    out: Bytes
    pause: bool
    broken: bool

    pub def write[S: ByteSource](self: &mut Drip, source: &S, start: i64, stop: i64) -> Result[i64, StreamError]:
        if self.broken:
            return Err(StreamError.Io(IoError.Other))
        self.pause = not self.pause
        if self.pause:
            return Err(StreamError.Interrupted)
        if start == stop:
            return Ok(0)
        match self.out.extend_range(source, start, start + 1):
            case Ok(_):
                Ok(1)
            case Err(_):
                Err(StreamError.OutOfRange)

    pub def flush(self: &mut Drip) -> Result[(), StreamError]:
        Ok(())

# A writer that counts bytes and keeps nothing, so it never allocates.
pub class Counter:
    total: i64

    pub def write[S: ByteSource](self: &mut Counter, source: &S, start: i64, stop: i64) -> Result[i64, StreamError]:
        self.total += stop - start
        Ok(stop - start)

    pub def flush(self: &mut Counter) -> Result[(), StreamError]:
        Ok(())

"#;

/// `The quick brown fox jumps over the lazy dog\n` encoded by CPython:
/// `gzip.compress(d, mtime=0)`, `zlib.compress(d)` and a raw `compressobj`.
const FOX: &str = "The quick brown fox jumps over the lazy dog";
const FOX_GZIP: &str = "1f8b08000000000002ff0bc94855282ccd4cce56482aca2fcf5348cbaf50c82acd2d2856c82f4b2d5228014ae72456552aa4e4a773010038c1936d2c000000";
const FOX_ZLIB: &str = "789c0bc94855282ccd4cce56482aca2fcf5348cbaf50c82acd2d2856c82f4b2d5228c94855c849acaa5448c94fe702006bc00fe4";
const FOX_RAW: &str =
    "0bc94855282ccd4cce56482aca2fcf5348cbaf50c82acd2d2856c82f4b2d5228014ae72456552aa4e4a7730100";
/// Two gzip members, `abc` and `def`.
const MEMBERS: &str =
    "1f8b08000000000002ff4b4c4a0600c2412435030000001f8b08000000000002ff4b494d030061e1c40c03000000";

fn program(body: &str) -> String {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    format!(
        "{HEADER}def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | std.encoding.EncodingError | std.compress.CompressError | std.io.Partial | std.io.StreamError | std.io.CopyError]:\n{body}    Ok(())\n"
    )
}

/// Runs the program with `input` on stdin and returns its raw stdout.
fn piped(source: &str, input: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let (_dir, executable) = build(source);
    let mut child = std::process::Command::new(executable)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let input = input.to_vec();
    let feeder = std::thread::spawn(move || stdin.write_all(&input));
    let output = child.wait_with_output().unwrap();
    feeder.join().unwrap().unwrap();
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn output(source: &str) -> String {
    let output = run(source);
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
        .replace("std.compress.CompressError.", "")
        .replace("std.compress.CompressError", "E")
        .replace("std.bytes.Bytes", "Bytes")
}

fn native(body: &str, expected: &str) {
    assert_eq!(
        output(&program(body)),
        expected.trim_matches('\n'),
        "{body}"
    );
}

/// Runs `command` with `input` on stdin, or `None` when it is not installed.
fn system(command: &str, args: &[&str], input: &[u8]) -> Option<Vec<u8>> {
    use std::io::Write;
    let mut child = std::process::Command::new(command)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .ok()?;
    let mut stdin = child.stdin.take().unwrap();
    let input = input.to_vec();
    let feeder = std::thread::spawn(move || stdin.write_all(&input));
    let output = child.wait_with_output().unwrap();
    feeder.join().unwrap().unwrap();
    assert!(output.status.success(), "{command} {args:?} failed");
    Some(output.stdout)
}

fn sample_text() -> Vec<u8> {
    (0..4000)
        .map(|i| format!("line {i}: {}\n", "abcdefghij".repeat(i % 7)))
        .collect::<String>()
        .into_bytes()
}

#[test]
fn external_fixtures_decode() {
    native(
        &format!(
            r#"
print(shown(std.compress.decompress(Format.Gzip, &unhex("{FOX_GZIP}")?, 100))?)?
print(shown(std.compress.decompress(Format.Zlib, &unhex("{FOX_ZLIB}")?, 100))?)?
print(shown(std.compress.decompress(Format.Deflate, &unhex("{FOX_RAW}")?, 100))?)?
print(shown(std.compress.decompress(Format.Gzip, &unhex("{MEMBERS}")?, 100))?)?
print(std.compress.decompress(Format.Gzip, &unhex("1f8b08000000000002ff03000000000000000000")?, 0)?.len())?
"#
        ),
        &format!("{FOX}\n\n{FOX}\n\n{FOX}\n\nabcdef\n0"),
    );
}

#[test]
fn round_trips_every_format_and_level() {
    native(
        r#"
text = repeated("compressible text ", 3000)?
random = noise(70000)?
empty = std.bytes.empty()?
for format in [Format.Deflate, Format.Zlib, Format.Gzip]?:
    for level in [0, 1, 6, 9]?:
        print(trip(format, level, &text)? and trip(format, level, &random)? and trip(format, level, &empty)?)?
small = std.compress.compress(Format.Gzip, &text, 9)?
print(small.len() < 200)?
stored = std.compress.compress(Format.Gzip, &random, 0)?
print(stored.len() > random.len())?
"#,
        &format!("{}True\nTrue", "True\n".repeat(12)),
    );
}

#[test]
fn checksums_truncation_and_trailing_data_are_errors() {
    let corrupt_crc = FOX_GZIP.replacen("0038c1936d", "0039c1936d", 1);
    let corrupt_length = format!("{}2d000000", &FOX_GZIP[..FOX_GZIP.len() - 8]);
    let corrupt_adler = format!("{}e5", &FOX_ZLIB[..FOX_ZLIB.len() - 2]);
    let truncated = &FOX_GZIP[..FOX_GZIP.len() - 8];
    native(
        &format!(
            r#"
print(shown(std.compress.decompress(Format.Gzip, &unhex("{corrupt_crc}")?, 100))?)?
print(shown(std.compress.decompress(Format.Gzip, &unhex("{corrupt_length}")?, 100))?)?
print(shown(std.compress.decompress(Format.Zlib, &unhex("{corrupt_adler}")?, 100))?)?
print(shown(std.compress.decompress(Format.Gzip, &unhex("{truncated}")?, 100))?)?
print(shown(std.compress.decompress(Format.Gzip, &unhex("")?, 100))?)?
print(shown(std.compress.decompress(Format.Gzip, &unhex("{FOX_GZIP}00")?, 100))?)?
print(shown(std.compress.decompress(Format.Zlib, &unhex("{FOX_ZLIB}{FOX_ZLIB}")?, 100))?)?
print(shown(std.compress.decompress(Format.Deflate, &unhex("{FOX_RAW}ff")?, 100))?)?
print(shown(std.compress.decompress(Format.Zlib, &unhex("{FOX_GZIP}")?, 100))?)?
print(shown(std.compress.decompress(Format.Gzip, &unhex("{FOX_GZIP}")?, -1))?)?
print(str.repr(std.compress.compress(Format.Gzip, &unhex("")?, 10))?)?
"#
        ),
        "Checksum\nChecksum\nChecksum\nTruncated\nTruncated\nTrailingData\nTrailingData\n\
         TrailingData\nCorrupt\nOutOfRange\nResult[Bytes, E].Err(OutOfRange)",
    );
}

#[test]
fn output_beyond_the_limit_is_rejected() {
    // 64 MiB of zeros packs into about 64 KiB; neither call may decode it all.
    native(
        r#"
zeros = repeated("\0", 65536)?
mut writer = std.compress.compressor(Format.Gzip, std.io.cursor(std.bytes.empty()?), 9)?
for _ in range(0, 1024):
    std.io.write_all(&mut writer, &zeros, 0, zeros.len())?
writer.finish()?
bomb = writer.inner.data.copy()?
print(bomb.len() < 100000)?
print(shown(std.compress.decompress(Format.Gzip, &bomb, 1000000))?)?
mut reader = std.compress.decompressor(Format.Gzip, std.io.cursor(bomb), 100000)?
mut out = std.bytes.empty()?
match std.io.read_up_to(&mut reader, &mut out, 10000000):
    case Ok(_):
        print("read everything")?
    case Err(partial):
        print(partial.completed <= 100000)?
        print(str.repr(partial.cause)?)?
print(str.repr(reader.failure())?)?
exact = std.compress.compress(Format.Zlib, &zeros, 6)?
print(std.compress.decompress(Format.Zlib, &exact, 65536)?.len())?
print(shown(std.compress.decompress(Format.Zlib, &exact, 65535))?)?
"#,
        "True\nLimitExceeded\nTrue\nstd.io.StreamError.Io(IoError.InvalidInput)\n\
         Option[E].Some(LimitExceeded)\n65536\nLimitExceeded",
    );
}

#[test]
fn system_gzip_reads_our_output_and_we_read_its_output() {
    let text = sample_text();
    let Some(packed) = system("gzip", &["-c", "-9"], &text) else {
        eprintln!("skipped: gzip is not installed");
        return;
    };
    // stdin through a decompressor to stdout, in 1000-byte pieces.
    let gunzip = program(
        r#"
mut reader = std.compress.decompressor(Format.Gzip, std.io.stdin(), 10000000)?
mut writer = std.io.stdout()
mut scratch = std.bytes.with_capacity(1000)?
scratch.resize(1000, 0u8)?
std.io.copy_all(&mut reader, &mut writer, &mut scratch)?
"#,
    );
    assert_eq!(piped(&gunzip, &packed), text);
    let gzip = program(
        r#"
mut writer = std.compress.compressor(Format.Gzip, std.io.stdout(), 6)?
mut reader = std.io.stdin()
mut scratch = std.bytes.with_capacity(777)?
scratch.resize(777, 0u8)?
std.io.copy_all(&mut reader, &mut writer, &mut scratch)?
writer.finish()?
"#,
    );
    let ours = piped(&gzip, &text);
    assert_eq!(system("gzip", &["-d", "-c"], &ours).unwrap(), text);
    // Concatenated members, as `cat a.gz b.gz` produces.
    let twice = [packed.as_slice(), ours.as_slice()].concat();
    let expected = [text.as_slice(), text.as_slice()].concat();
    assert_eq!(piped(&gunzip, &twice), expected);
}

#[test]
fn short_and_interrupted_io_preserves_the_stream() {
    native(
        r#"
text = repeated("short reads and writes ", 500)?
mut writer = std.compress.compressor(Format.Zlib, Drip(std.bytes.empty()?, False, False), 6)?
std.io.write_all(&mut writer, &text, 0, 5000)?
writer.flush()?
std.io.write_all(&mut writer, &text, 5000, text.len())?
writer.inner.broken = True
print(str.repr(writer.finish())?)?
writer.inner.broken = False
writer.finish()?
writer.finish()?
print(str.repr(writer.write(&text, 0, 1))?)?
packed = writer.inner.out.copy()?
mut reader = std.compress.decompressor(Format.Zlib, Trickle(packed, 0, False), 100000)?
mut out = std.bytes.empty()?
print(std.io.read_up_to(&mut reader, &mut out, 100000)?)?
print(hex(&out)? == hex(&text)?)?
print(str.repr(reader.failure())?)?
"#,
        "Result[(), std.io.StreamError].Err(std.io.StreamError.Io(IoError.Other))\n\
         Result[i64, std.io.StreamError].Err(std.io.StreamError.Io(IoError.Closed))\n\
         11500\nTrue\nOption[E].Nothing",
    );
}

#[test]
fn a_sync_flush_makes_written_data_decodable() {
    native(
        r#"
text = std.bytes.from_text("first part")?
mut writer = std.compress.compressor(Format.Deflate, std.io.cursor(std.bytes.empty()?), 6)?
std.io.write_all(&mut writer, &text, 0, text.len())?
writer.flush()?
mut reader = std.compress.decompressor(Format.Deflate, std.io.cursor(writer.inner.data.copy()?), 100)?
mut out = std.bytes.empty()?
match std.io.read_up_to(&mut reader, &mut out, 100):
    case Ok(_):
        print("complete")?
    case Err(partial):
        print(str.repr(partial.cause)?)?
print(out.to_text()?)?
print(str.repr(reader.failure())?)?
"#,
        "std.io.StreamError.UnexpectedEof\nfirst part\nOption[E].Some(Truncated)",
    );
}

#[test]
fn a_failed_reader_stays_failed() {
    let corrupt = FOX_GZIP.replacen("0038c1936d", "0039c1936d", 1);
    native(
        &format!(
            r#"
mut reader = std.compress.decompressor(Format.Gzip, Trickle(unhex("{corrupt}")?, 0, False), 100)?
mut out = std.bytes.empty()?
match std.io.read_up_to(&mut reader, &mut out, 100):
    case Ok(_):
        print("complete")?
    case Err(partial):
        print(str.repr(partial.cause)?)?
# Fed a byte at a time, the text arrives before the trailer that proves it
# wrong.
print(out.to_text()?)?
mut buffer = std.bytes.with_capacity(8)?
buffer.resize(8, 0u8)?
print(str.repr(reader.read(&mut buffer, 0, 8))?)?
print(str.repr(reader.failure())?)?
"#
        ),
        &format!(
            "std.io.StreamError.Io(IoError.InvalidInput)\n{FOX}\n\n\
             Result[i64, std.io.StreamError].Err(std.io.StreamError.Io(IoError.InvalidInput))\n\
             Option[E].Some(Checksum)"
        ),
    );
}

#[test]
fn crc32_matches_the_check_value() {
    native(
        r#"
data = std.bytes.from_text("123456789")?
print(std.compress.crc32(0, &data, 0, 9)?)?
first = std.compress.crc32(0, &data, 0, 4)?
print(std.compress.crc32(first, &data, 4, 9)?)?
print(str.repr(std.compress.crc32(0, &data, 0, 10))?)?
print(str.repr(std.compress.crc32(-1, &data, 0, 1))?)?
"#,
        "3421780262\n3421780262\nResult[i64, E].Err(OutOfRange)\nResult[i64, E].Err(OutOfRange)",
    );
}

// Issue #238: a zero-capacity state could never make step or flush progress.
#[test]
fn codec_states_need_output_room() {
    native(
        r#"
print(str.repr(std.bytes.compress_state(1u8, 6, 0))?)?
print(str.repr(std.bytes.compress_state(1u8, 6, -1))?)?
print(std.bytes.compress_state(1u8, 6, 1).is_ok())?
"#,
        "Result[Bytes, BytesError].Err(BytesError.OutOfRange)\nResult[Bytes, BytesError].Err(BytesError.OutOfRange)\nTrue",
    );
}

#[test]
fn codec_states_cannot_be_forged() {
    let output = run(&program(
        "mut state = std.bytes.from_text(\"not a state\")?\n\
         data = std.bytes.from_text(\"abc\")?\n\
         print(state.compress_step(&data, 0, 3)?)?\n",
    ));
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("operand is not a std.compress state"),
        "{stderr}"
    );
}

#[test]
fn brotli_round_trips_all_qualities_binary_empty_and_large_output() {
    native(
        r#"
empty = std.bytes.empty()?
data = noise(40000)?
text = repeated("Brotli is a streaming format. ", 10000)?
for quality in range(12):
    print(trip(Format.Brotli, quality, &empty)?, trip(Format.Brotli, quality, &data)?, trip(Format.Brotli, quality, &text)?)?
print(shown(std.compress.compress(Format.Brotli, &empty, -1))?)?
print(shown(std.compress.compress(Format.Brotli, &empty, 12))?)?
print(shown(std.compress.compress(Format.Gzip, &empty, 10))?)?
"#,
        &format!(
            "{}\nOutOfRange\nOutOfRange\nOutOfRange",
            vec!["True True True"; 12].join("\n")
        ),
    );
}

#[test]
fn brotli_independent_fixtures_truncation_trailing_data_and_exact_limit() {
    // Generated independently by Python Brotli 1.2.0, quality 11. The second
    // fixture consumes its compressed input before all 100,000 output bytes
    // fit the 16 KiB output window; an empty-input drain is essential.
    native(
        r#"
fox = unhex("8b158054686520717569636b2062726f776e20666f78206a756d7073206f76657220746865206c617a7920646f670a03")?
print(std.compress.decompress(Format.Brotli, &fox, 44)?.to_text()?)?
packed = unhex("5b9f86815f22281e0b04b2fc0200")?
plain = std.compress.decompress(Format.Brotli, &packed, 100000)?
print(plain.len(), plain.get(99999)?)?
print(shown(std.compress.decompress(Format.Brotli, &packed, 99999))?)?
for stop in range(packed.len()):
    prefix = packed.slice(0, stop)?
    print(shown(std.compress.decompress(Format.Brotli, &prefix, 100000))?)?
mut trailing = packed.copy()?
trailing.push(0u8)?
print(shown(std.compress.decompress(Format.Brotli, &trailing, 100000))?)?
mut concatenated = packed.copy()?
concatenated.extend(&packed)?
print(shown(std.compress.decompress(Format.Brotli, &concatenated, 200000))?)?
invalid = unhex("ff")?
print(shown(std.compress.decompress(Format.Brotli, &invalid, 100000))?)?
"#,
        &format!(
            "The quick brown fox jumps over the lazy dog\n\n100000 65\nLimitExceeded\n{}\nTrailingData\nTrailingData\nCorrupt",
            vec!["Truncated"; 14].join("\n")
        ),
    );
}

#[test]
fn brotli_short_io_flush_finish_retry_and_final_output_drain() {
    native(
        r#"
text = repeated("short reads and writes ", 500)?
mut writer = std.compress.compressor(Format.Brotli, Drip(std.bytes.empty()?, False, False), 5)?
std.io.write_all(&mut writer, &text, 0, 5000)?
writer.flush()?
std.io.write_all(&mut writer, &text, 5000, text.len())?
writer.inner.broken = True
print(str.repr(writer.finish())?)?
writer.inner.broken = False
writer.finish()?
writer.finish()?
print(str.repr(writer.write(&text, 0, 1))?)?
packed = writer.inner.out.copy()?
mut reader = std.compress.decompressor(Format.Brotli, Trickle(packed, 0, False), 100000)?
mut out = std.bytes.empty()?
print(std.io.read_up_to(&mut reader, &mut out, 100000)?)?
print(hex(&out)? == hex(&text)?)?
print(str.repr(reader.failure())?)?
large = unhex("5b9f86815f22281e0b04b2fc0200")?
mut expanding = std.compress.decompressor(Format.Brotli, Trickle(large, 0, False), 100000)?
mut expanded = std.bytes.empty()?
print(std.io.read_up_to(&mut expanding, &mut expanded, 100001)?, expanded.get(99999)?)?
"#,
        "Result[(), std.io.StreamError].Err(std.io.StreamError.Io(IoError.Other))\nResult[i64, std.io.StreamError].Err(std.io.StreamError.Io(IoError.Closed))\n11500\nTrue\nOption[E].Nothing\n100000 65",
    );
}

#[test]
fn brotli_sync_flush_and_sticky_reader_failure() {
    native(
        r#"
text = std.bytes.from_text("first part")?
mut writer = std.compress.compressor(Format.Brotli, std.io.cursor(std.bytes.empty()?), 5)?
std.io.write_all(&mut writer, &text, 0, text.len())?
writer.flush()?
mut reader = std.compress.decompressor(Format.Brotli, std.io.cursor(writer.inner.data.copy()?), 100)?
mut out = std.bytes.empty()?
match std.io.read_up_to(&mut reader, &mut out, 100):
    case Ok(_):
        print("wrong")?
    case Err(partial):
        print(str.repr(partial.cause)?)?
print(out.to_text()?)?
print(str.repr(reader.failure())?)?
mut scratch = std.bytes.with_capacity(1)?
scratch.resize(1, 0u8)?
print(str.repr(reader.read(&mut scratch, 0, 1))?)?
"#,
        "std.io.StreamError.UnexpectedEof\nfirst part\nOption[E].Some(Truncated)\nResult[i64, std.io.StreamError].Err(std.io.StreamError.UnexpectedEof)",
    );
}

#[test]
fn brotli_malformed_inputs_are_bounded() {
    native(
        r#"
fixture = unhex("5b9f86815f22281e0b04b2fc0200")?
for index in range(fixture.len()):
    for bit in range(8):
        mut changed = fixture.copy()?
        original = changed.get(index)?
        changed.set(index, original ^ (1u8 << u8(bit)))?
        drop(std.compress.decompress(Format.Brotli, &changed, 4096))
for size in range(256):
    data = noise(size)?
    drop(std.compress.decompress(Format.Brotli, &data, 4096))
print("bounded")?
"#,
        "bounded",
    );
}

#[test]
fn python_brotli_and_plenty_read_each_others_streams() {
    let available = std::process::Command::new("python3")
        .args(["-c", "import brotli"])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if !available {
        eprintln!(
            "skipped live Brotli interop: Python brotli is unavailable; fixed fixtures still run"
        );
        return;
    }
    let data = sample_text();
    let packed = system("python3", &["-c", "import brotli,sys;sys.stdout.buffer.write(brotli.compress(sys.stdin.buffer.read(),quality=9))"], &data).unwrap();
    let decoder = program(
        r#"
mut reader = std.compress.decompressor(Format.Brotli, std.io.stdin(), 10000000)?
mut writer = std.io.stdout()
mut scratch = std.bytes.with_capacity(777)?
scratch.resize(777, 0u8)?
std.io.copy_all(&mut reader, &mut writer, &mut scratch)?
"#,
    );
    assert_eq!(piped(&decoder, &packed), data);
    let encoder = program(
        r#"
mut reader = std.io.stdin()
mut writer = std.compress.compressor(Format.Brotli, std.io.stdout(), 5)?
mut scratch = std.bytes.with_capacity(1000)?
scratch.resize(1000, 0u8)?
std.io.copy_all(&mut reader, &mut writer, &mut scratch)?
writer.finish()?
"#,
    );
    let ours = piped(&encoder, &data);
    assert_eq!(system("python3", &["-c", "import brotli,sys;sys.stdout.buffer.write(brotli.decompress(sys.stdin.buffer.read()))"], &ours).unwrap(), data);
}

#[cfg(feature = "runtime-checks")]
#[test]
fn brotli_allocation_failures_and_partial_stream_drops_are_leak_free() {
    let mut helpers = String::new();
    let mut body = String::from(
        "data = repeated(\"allocation failure \", 500)?\npacked = std.compress.compress(Format.Brotli, &data, 5)?\n",
    );
    for budget in 0..100 {
        body.push_str(&format!("probe{budget}(&data, &packed)?\n"));
        helpers.push_str(&format!(r#"
def probe{budget}(data: &Bytes, packed: &Bytes) -> Result[(), AllocError | IoError | std.bytes.BytesError | CompressError | StreamError | std.io.Partial]:
    print("__test_fail_allocations_after_{budget}__")?
    encoded = std.compress.compress(Format.Brotli, data, 5)
    print("__test_restore_allocations__")?
    match encoded:
        case Ok(value):
            print("encoded")?
        case Err(error):
            print(error)?
    print("__test_fail_allocations_after_{budget}__")?
    decoded = std.compress.decompress(Format.Brotli, packed, 10000)
    print("__test_restore_allocations__")?
    match decoded:
        case Ok(value):
            print("decoded")?
        case Err(error):
            print(error)?
    mut unfinished = std.compress.compressor(Format.Brotli, std.io.cursor(std.bytes.empty()?), 5)?
    std.io.write_all(&mut unfinished, data, 0, data.len())?
    mut partial = std.compress.decompressor(Format.Brotli, std.io.cursor(packed.copy()?), 10000)?
    mut scratch = std.bytes.with_capacity(1)?
    scratch.resize(1, 0u8)?
    drop(partial.read(&mut scratch, 0, 1)?)
    Ok(())
"#));
    }
    let out = output(&format!("{}\n{helpers}", program(&body)));
    assert_eq!(out.lines().count(), 200);
    assert!(out.contains("encoded") && out.contains("decoded") && out.contains("OutOfMemory"));
    for line in out.lines() {
        assert!(
            line == "encoded" || line == "decoded" || line == "Allocation(AllocError.OutOfMemory)",
            "{line}"
        );
    }
}

#[test]
fn compressing_works_in_worker_threads() {
    let source = format!(
        r#"{HEADER}
def work(text: str, format: Format) -> Result[(), AllocError | IoError | std.bytes.BytesError | std.compress.CompressError]:
    data = std.bytes.from_text(text)?
    packed = std.compress.compress(format, &data, 6)?
    print(std.compress.decompress(format, &packed, 100)?.to_text()?)?
    Ok(())

def main() -> Result[(), AllocError | IoError | ThreadError]:
    with spawn(work, "from a thread", Format.Gzip)?:
        pass
    with spawn(work, "Brotli from a thread", Format.Brotli)?:
        pass
    Ok(())
"#
    );
    assert_eq!(output(&source), "from a thread\nBrotli from a thread");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn streaming_after_setup_does_not_allocate() {
    // The compressor, decompressor and their buffers are allocated before the
    // checked region; writing, finishing and reading 2 MiB allocate nothing.
    native(
        r#"
text = repeated("steady state ", 4096)?
mut writer = std.compress.compressor(Format.Gzip, Counter(0), 6)?
mut packed = std.io.cursor(std.bytes.empty()?)
mut copier = std.compress.compressor(Format.Gzip, packed, 6)?
for _ in range(0, 40):
    std.io.write_all(&mut copier, &text, 0, text.len())?
copier.finish()?
mut reader = std.compress.decompressor(Format.Gzip, std.io.cursor(copier.inner.data.copy()?), 10000000)?
mut buffer = std.bytes.with_capacity(4096)?
buffer.resize(4096, 0u8)?
mut total = 0
print("__test_begin_no_allocations__")?
for _ in range(0, 40):
    std.io.write_all(&mut writer, &text, 0, text.len())?
writer.finish()?
while True:
    count = reader.read(&mut buffer, 0, 4096)?
    if count == 0:
        break
    total += count
print("__test_end_no_allocations__")?
print(writer.inner.total < 100000)?
print(total)?
"#,
        "True\n2129920",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_is_reported_without_leaks() {
    // Each failure point in creation (output buffer, owner, codec state) is
    // reported as an error; the exit-time balance check verifies that the
    // partial states, and the unfinished streams dropped at the end, leak
    // nothing.
    for after in 0..6 {
        let body = format!(
            r#"
data = repeated("allocation ", 100)?
sink = std.io.cursor(std.bytes.empty()?)
source = std.io.cursor(std.bytes.empty()?)
print("__test_fail_allocations_after_{after}__")?
packed = std.compress.compress(Format.Gzip, &data, 6)
writer = std.compress.compressor(Format.Zlib, sink, 1)
reader = std.compress.decompressor(Format.Deflate, source, 10)
print("__test_restore_allocations__")?
match packed:
    case Ok(bytes):
        print(std.compress.decompress(Format.Gzip, &bytes, 2000)?.len())?
    case Err(error):
        print(str.repr(error)?)?
mut unfinished = std.compress.compressor(Format.Gzip, std.io.cursor(std.bytes.empty()?), 6)?
std.io.write_all(&mut unfinished, &data, 0, data.len())?
mut partial = std.compress.decompressor(Format.Gzip, std.io.cursor(std.compress.compress(Format.Gzip, &data, 6)?), 2000)?
mut buffer = std.bytes.with_capacity(10)?
buffer.resize(10, 0u8)?
print(partial.read(&mut buffer, 0, 10)?)?
"#
        );
        let out = output(&program(&body));
        let mut lines = out.lines();
        let first = lines.next().unwrap();
        // The first three allocations are the codec's own: a failure in
        // any of them, including zlib-rs's state, must be reported.
        let expected = if after < 3 {
            "Allocation(AllocError.OutOfMemory)"
        } else {
            first
        };
        assert_eq!(first, expected, "after {after}: {out}");
        assert!(
            first == "1100" || first == "Allocation(AllocError.OutOfMemory)",
            "after {after}: {out}"
        );
        assert_eq!(lines.next(), Some("10"), "after {after}: {out}");
    }
}
