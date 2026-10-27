# std.compress

`import std.compress` compresses and decompresses DEFLATE (RFC 1951), zlib
(RFC 1950), gzip (RFC 1952) and Brotli (RFC 7932) data, whole [`std.bytes`](bytes.md) buffers at
once or as [`std.io`](io.md) streams. It has one error type, `CompressError`.

DEFLATE codecs and CRC-32 use `zlib-rs`; [Brotli](compress-brotli.md) uses the
reference codec with fallible allocation callbacks, bundled into the runtime.
The Plenty module owns buffering, the output limit, and the policy for input
that follows a stream. The module is available on the supported x86-64 Linux
GNU target.

```plenty
import std.bytes
import std.compress
from std.compress import Format

def main() -> Result[(), IoError | std.bytes.BytesError | std.compress.CompressError]:
    mut log = std.bytes.empty()?
    for _ in range(50):
        log.extend(&std.bytes.from_text("GET /index.html 200\n")?)?
    packed = std.compress.compress(Format.Gzip, &log, 6)?
    print(log.len(), packed.len() < log.len())?
    restored = std.compress.decompress(Format.Gzip, &packed, 4096)?
    print(restored == log)?
    Ok(())
```
```output
1000 True
True
```

## Functions

| Function | Result | Contract |
|---|---|---|
| `compress(format: Format, data: &Bytes, level: i64)` | `Result[Bytes, CompressError]` | All of `data`, compressed |
| `decompress(format: Format, data: &Bytes, limit: i64)` | `Result[Bytes, CompressError]` | The contents of `data`, at most `limit` bytes |
| `compressor(format: Format, inner: W, level: i64)` | `Result[Compressor[W], CompressError]` | A writer that compresses into `inner` |
| `decompressor(format: Format, inner: R, limit: i64)` | `Result[Decompressor[R], CompressError]` | A reader of the decompressed contents of `inner` |
| `inflater(format: Format, limit: i64)` | `Result[Inflater, CompressError]` | A step-by-step decoder; see [Inflater](#inflater) |
| `crc32(seed: i64, data: &Bytes, start: i64, stop: i64)` | `Result[i64, CompressError]` | The CRC-32 of `data[start:stop]`, continuing from `seed` |
| `stream_error(error: CompressError)` | `StreamError` | How a `Decompressor` reports `error` |

`Format` is `Deflate` (raw, no header or checksum), `Zlib` (Adler-32),
`Gzip` (CRC-32 and length), or `Brotli` (no checksum). For DEFLATE formats,
`level` runs from 0, which stores the data
uncompressed in blocks, to 9, which is smallest and slowest; 6 is a usual
choice. Brotli takes qualities 0–11; 5 is a useful starting point, while 10–11
spend substantially more time and memory. A level outside its format's range
or a negative `limit` is `OutOfRange`.

`crc32` is the gzip and zip checksum; start a fresh sum with seed 0. The
check value of the nine bytes `123456789` is 3421780262. `seed` must lie in
0 to 2³²−1.

## Input policy

A decoder accepts exactly one stream, with these rules:

- **Truncation.** Input that ends inside a stream is `Truncated`.
- **Concatenated gzip members.** When a gzip member ends and the next byte is
  31 (the first byte of the gzip magic), the next member is decoded and its
  output appended, as `gzip -d` does for `cat a.gz b.gz`. Each member's CRC
  and length are checked.
- **Trailing data.** Any other byte after the end of a stream is
  `TrailingData`, including the zero padding some tools append to gzip files.
  Raw DEFLATE, zlib and Brotli streams never continue.
- **Integrity.** A gzip CRC-32 or length mismatch, a zlib Adler-32 mismatch,
  and a gzip header CRC mismatch are `Checksum`. Raw DEFLATE and Brotli have no checksum.
  Any other malformed input, a zlib stream that needs a preset dictionary, or
  a stream of another format is `Corrupt`.
- **Output limit.** Producing more than `limit` bytes in total, counted across
  gzip members, is `LimitExceeded`. Exactly `limit` bytes succeeds. Choose the
  limit from what the program can hold, not from the input size: a few
  kilobytes of DEFLATE can expand by a factor of 1000.

Gzip header fields (name, comment, modification time, extra data) are
validated and discarded; the encoder writes a minimal header with no name and
time 0, so equal input gives equal output.

## Compressor

`Compressor[W]` has the [`Writer`](io.md#protocols) methods, so `write_all`,
`copy_all`, and anything else generic over `Writer` accept it. `inner` is
public.

| Method | Result | Contract |
|---|---|---|
| `write[S: ByteSource](source: &S, start: i64, stop: i64)` | `Result[i64, StreamError]` | Compress part of `source[start:stop]`, a `Bytes` or a `b"..."` literal; the count is the input consumed |
| `flush()` | `Result[(), StreamError]` | Deliver everything written so far as complete blocks, then flush `inner` |
| `finish()` | `Result[(), StreamError]` | Write the stream's end and checksum, then flush `inner` |

Output reaches `inner` in order, on later `write` calls and in `flush` and
`finish`. A `write` first delivers pending output from the previous one; if
that fails, the error is returned and none of `source` is consumed. A failure
keeps the undelivered bytes, so calling the method again resumes where it
stopped. Interrupted inner writes are retried; a zero-byte inner write is
`WriteZero`.

Call `finish` before using the result. Dropping a compressor without it
leaves `inner` holding an incomplete stream, which a decoder reports as
`Truncated`. After `finish`, `write` is `StreamError.Io(IoError.Closed)` and
`finish` and `flush` only deliver what is left. Each `flush` ends the current
block, which costs some compression ratio; flush at message boundaries, not
after every write.

## Decompressor

`Decompressor[R]` has the [`Reader`](io.md#protocols) method `read(buffer,
start, stop)`, which returns at least one decoded byte, or 0 once the stream
and the input have both ended. `inner` is public.

| Method | Result | Contract |
|---|---|---|
| `read(buffer: &mut Bytes, start: i64, stop: i64)` | `Result[i64, StreamError]` | Decode into `buffer[start:stop]` |
| `failure()` | `Option[CompressError]` | The decoding failure that ended the stream, if any |

An error from `inner` is returned unchanged and leaves the reader usable, so
`Interrupted` and other transient failures may be retried. A decoding failure
ends the stream: `read` returns it through the `Reader` protocol, and every
later call repeats it. `failure` gives the precise cause, which
`stream_error` maps as follows:

| `CompressError` | `StreamError` |
|---|---|
| `Truncated` | `UnexpectedEof` |
| `Allocation(cause)` | `Allocation(cause)` |
| `OutOfRange` | `OutOfRange` |
| `Corrupt`, `Checksum`, `TrailingData`, `LimitExceeded` | `Io(IoError.InvalidInput)` |

gzip and zlib store their checksum after the data, so a reader may already
have returned bytes when it finds a `Checksum` failure. Treat decoded output
as unverified until `read` has returned 0. Bytes decoded in the step that
fails are discarded. `decompress` returns nothing on any error.

## Inflater

`Inflater` is the decoder inside `Decompressor`, for a reader that owns its
input and cannot hand over an `inner` stream, as
[`std.archive`](archive.md) does for ZIP entries. It applies the same
[input policy](#input-policy) and limit.

| Method | Result | Contract |
|---|---|---|
| `step(input: &Bytes, start: i64, stop: i64)` | `Result[i64, CompressError]` | Decode from `input[start:stop]`; returns the count consumed |
| `output()` | `&Bytes` | The output of the last step, at most 16384 bytes |
| `clear()` | `()` | Empty the pending output |
| `complete()` | `bool` | Whether the input so far is a complete stream |

Empty the output with `clear` before each `step`. After consuming the input,
call `step` with `start == stop` until `complete()` or a step produces no output:
Brotli can have pending decoded bytes after consuming its final input byte.
A step that returns 0 with no output needs more input unless complete. Discard
the `Inflater` after any error; do not rely on retries of codec errors.

## Errors

```text
pub enum CompressError:
    OutOfRange
    Corrupt
    Checksum
    Truncated
    TrailingData
    LimitExceeded
    Allocation(AllocError)
```

| Variant | Meaning |
|---|---|
| `OutOfRange` | A level, limit, seed, or range out of bounds |
| `Corrupt` | Malformed compressed data |
| `Checksum` | Well-formed data whose checksum or length does not match |
| `Truncated` | The input ended inside a stream |
| `TrailingData` | Bytes after the end of the stream; see [Input policy](#input-policy) |
| `LimitExceeded` | More than `limit` bytes of output |
| `Allocation(cause)` | A codec state, buffer, or output could not be allocated |

## Ownership and threads

A `Compressor` or `Decompressor` owns its `inner` stream, its codec state,
and its buffers; dropping it releases them all, finished or not, and does not
flush or close `inner`. Neither can be copied. The module has no global state,
passes the ordinary worker-effect checks, and may be used from any thread;
moving a stream to a worker moves its ownership.

The codec state is held in a `Bytes` whose contents are the pending output.
The back-end methods `compress_step`, `compress_flush`, and their siblings
are public on `Bytes` only so that `std.compress` can call them. On a buffer
not made by `std.compress` they stop the program with
`operand is not a std.compress state`.

## Costs

For the DEFLATE formats, memory is fixed when a stream is made and does not
grow with the data. Brotli's different allocation contract is described
[separately](compress-brotli.md#memory-and-failures):

| Value | Allocates |
|---|---|
| `Compressor` | 371 KiB of zlib-rs state at every level (window, hash chains, symbol buffer) and a 16 KiB output buffer |
| `Decompressor` | 46 KiB of zlib-rs state (including the 32 KiB window), a 16 KiB output buffer, and a 16 KiB input buffer |
| `compress`, `decompress` | One codec state and buffer as above, and the result, which grows by doubling |

| Operation | Allocates | Time |
|---|---|---|
| `Compressor.write`, `flush`, `finish` | Nothing | Linear in the input; higher levels are slower |
| `Decompressor.read` | Nothing | Linear in the output |
| `crc32` | Nothing | Linear in the range |

Inputs are read directly from the caller's buffer; output is copied once,
from the codec's buffer to `inner` or the caller's buffer. Allocation failures
are reported as `Allocation`; dropping a failed or unfinished codec releases
its state and buffers.

## Supply

The runtime builds `zlib-rs` from an unmodified copy under
`plenty-runtime/vendor`, so building the compiler needs no network and an
installed compiler needs neither the sources nor a Rust toolchain:

| Crate | Version | Licence | Maintainer |
|---|---|---|---|
| `zlib-rs` | 0.6.8 | Zlib | Trifecta Tech Foundation, as part of `zlib-rs` / `libz-rs-sys` |

`zlib-rs` has no dependencies. It is a Rust port of zlib with zlib-ng's
optimisations, and reports allocation failure as an error rather than
aborting, which `Allocation` relies on. The runtime calls its zlib-style
interface (`deflate::init`, `inflate::inflate`, and so on), enabled by the
crate's `__internal-api` feature.

The licence file is kept with the sources. The version is pinned in
`Cargo.lock`, and a test fails if the two disagree. Updating it is the
runtime maintainers' job, with each release that fixes a defect: replace the
directory with the new release, run `cargo update -p zlib-rs --precise
VERSION`, and rerun `tests/test_std_compress.rs`, whose fixtures come from
CPython's `zlib` and `gzip` and from the system `gzip` tool. The
`__internal-api` interface is not covered by semver, so read the release's
changes to `deflate.rs` and `inflate.rs`.

Executables are linked with `--gc-sections`, and the codecs are reached only
through their own runtime entry symbols, so a program that does not use
`std.compress` contains none of them.

## Limits

- No bzip2, xz, zstd, or lz4. ZIP and TAR are in
  [`std.archive`](archive.md).
- No preset dictionaries, compression strategies, or window and memory
  settings; DEFLATE encoders use a 32 KiB window and zlib's default memory
  level, and Brotli uses the standard 4 MiB encoder window.
- No access to gzip header fields.
- Trailing padding after gzip members is an error, not ignored.
- No seeking within compressed data.
- One target, x86-64 Linux GNU.

## Acknowledgements

- Wraps [zlib-rs](https://github.com/trifectatechfoundation/zlib-rs) 0.6.8, vendored in the runtime (Zlib).
