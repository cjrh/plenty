//! `std.compress`: DEFLATE, zlib and gzip through zlib-rs, checked against
//! fixtures from CPython's zlib and gzip modules and against the system
//! `gzip` tool.

const HEADER: &str = r#"import std.bytes
import std.compress
import std.encoding
import std.io
from std.bytes import Bytes
from std.compress import Format, CompressError
from std.io import StreamError

def hex(data: &Bytes) -> Result[str, Failure]:
    Ok(std.encoding.hex_text(data, 0, data.len())?)

def unhex(text: &str) -> Result[Bytes, Failure]:
    Ok(std.encoding.hex_from_text(text)?)

# The decoded text, or the error's name.
def shown(result: Result[Bytes, CompressError]) -> Result[str, Failure]:
    match result:
        case Ok(data):
            Ok(data.to_text()?)
        case Err(error):
            Ok(str.repr(error)?)

def repeated(text: &str, count: i64) -> Result[Bytes, Failure]:
    piece = std.bytes.from_text(text)?
    mut out = std.bytes.with_capacity(piece.len() * count)?
    for _ in range(0, count):
        out.extend(&piece)?
    Ok(out)

# Deterministic bytes that do not compress.
def noise(count: i64) -> Result[Bytes, Failure]:
    mut out = std.bytes.with_capacity(count)?
    mut x = 88172645463325252
    for _ in range(0, count):
        x = x ^ (x << 13)
        x = x ^ (x >> 7)
        x = x ^ (x << 17)
        out.push(u8.wrapping_from(x >> 24))?
    Ok(out)

def trip(format: Format, level: i64, data: &Bytes) -> Result[bool, Failure]:
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

    pub def write(self: &mut Drip, source: &Bytes, start: i64, stop: i64) -> Result[i64, StreamError]:
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

    pub def write(self: &mut Counter, source: &Bytes, start: i64, stop: i64) -> Result[i64, StreamError]:
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
    format!("{HEADER}def main() -> Result[(), Failure]:\n{body}    Ok(())\n")
}

fn build(source: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    (dir, executable)
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

fn run(source: &str) -> std::process::Output {
    let (_dir, executable) = build(source);
    std::process::Command::new(executable).output().unwrap()
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
fn compressing_works_in_worker_threads() {
    let source = format!(
        r#"{HEADER}
def work(text: str) -> Result[(), Failure]:
    data = std.bytes.from_text(text)?
    packed = std.compress.compress(Format.Gzip, &data, 6)?
    print(std.compress.decompress(Format.Gzip, &packed, 100)?.to_text()?)?
    Ok(())

def main() -> Result[(), Failure]:
    with spawn(work, "from a thread")?:
        pass
    Ok(())
"#
    );
    assert_eq!(output(&source), "from a thread");
}

/// Every program links with `--gc-sections`, and the codecs sit behind their
/// own runtime entry, so only programs that compress carry zlib-rs.
#[test]
fn programs_that_never_compress_do_not_link_the_codecs() {
    use object::{Object, ObjectSymbol, SymbolKind};
    let symbols = |source: &str| {
        let (_dir, executable) = build(source);
        let data = std::fs::read(&executable).unwrap();
        let file = object::File::parse(&*data).unwrap();
        let count = file
            .symbols()
            // The linker keeps a file symbol for every archive member it
            // opened, including ones whose sections were all discarded.
            .filter(|symbol| symbol.kind() != SymbolKind::File)
            .filter_map(|symbol| symbol.name().ok().map(str::to_owned))
            .filter(|name| name == "plenty_compress" || name.contains("zlib_rs"))
            .count();
        (count, data.len())
    };
    let plain = "def main() -> Result[(), Failure]:\n    print(\"hi\")?\n    Ok(())\n";
    let (count, plain_size) = symbols(plain);
    assert_eq!(count, 0);
    let (count, size) = symbols(&program(
        "print(std.compress.compress(Format.Gzip, &std.bytes.empty()?, 6)?.len())?\n",
    ));
    assert!(count > 0);
    assert!(size > plain_size);
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
