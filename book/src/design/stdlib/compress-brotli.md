# Brotli streams

Use `std.compress.Format.Brotli` with the same whole-buffer functions,
`Compressor`, `Decompressor` and `Inflater` as the
[other compression formats](compress.md). The format is
[RFC 7932](https://www.rfc-editor.org/rfc/rfc7932.html), also used by HTTP's
`Content-Encoding: br`.

```plenty
import std.bytes
import std.compress
from std.compress import Format, CompressError
from std.bytes import BytesError

def main() -> Result[(), BytesError | CompressError | IoError]:
    original = std.bytes.from_text("A Brotli message")?
    encoded = std.compress.compress(Format.Brotli, &original, 5)?
    decoded = std.compress.decompress(Format.Brotli, &encoded, 1024)?
    print(decoded.to_text()?)?
    Ok(())
```
```output
A Brotli message
```

Quality ranges from 0 (fastest) to 11 (most effort). It is a compression-effort
setting, not a guarantee that each higher setting produces a smaller result.
Start with 5 for general use; choose 10–11 when extra compression work is
acceptable, such as preparing static assets. Quality 0 still compresses; it
does not mean the stored-block mode of DEFLATE level 0. The encoder uses the
standard 4 MiB window. The decoder accepts standard windows up to 16 MiB;
the nonstandard large-window extension and custom dictionaries are disabled.

`flush()` makes all written input decodable without ending the stream.
`finish()` writes its final marker; repeated successful finishes are harmless.
Short/interrupted inner I/O and retrying an inner write failure preserve pending
output. Codec failures, including allocation failures, require discarding that
codec. A whole-buffer call returns no partial value on any error.

The decoder rejects trailing data, including a second Brotli stream. Only gzip
has the concatenated-member policy. Truncated input is `Truncated`; malformed
input is `Corrupt`. Brotli has no checksum: a changed stream can still be valid
and decode to changed data. Use transport/application integrity when required.

## Memory and failures

The Plenty input/output staging buffers remain 16 KiB. Unlike the DEFLATE
implementation, Brotli allocates codec working memory during `write`, `read`,
`flush` and `finish`. Working memory depends on quality, stream window and block
structure; a decoder may allocate its history before hitting the output limit.
The output limit is not a total-memory quota. It applies to all produced output,
including bytes buffered internally after the last input byte has been consumed.

The native callbacks use Plenty's fallible allocator and matching deallocator,
so failure is `CompressError.Allocation(AllocError.OutOfMemory)` (or its
`StreamError.Allocation` projection), not process termination. Failed and
unfinished states release every native allocation on drop. There is no
allocation-free-after-setup guarantee for Brotli.

The low-level `Inflater` caller must drain output even when all input has been
consumed: clear its output, call `step(input, stop, stop)`, and repeat until
complete or no output is produced. `decompress` and `Decompressor` do this
automatically. An incomplete decoder with no more input/output is truncated.

## HTTP

The HTTP client continues to expose the encoded entity body; it does not
negotiate or decode content automatically. If your request sends
`Accept-Encoding: br`, check the actual response's `Content-Encoding` and use
`decompress(Format.Brotli, &fetched.body, decoded_limit)` for a single `br`
coding. Bound the encoded HTTP body and the decoded output independently.
Multiple content codings must be decoded in reverse application order; do not
assume every response is Brotli merely because the request accepted it.

Servers can compress response bytes with the same function and send
`Content-Encoding: br`. The content length describes the compressed bytes.
Choose a representation from the request's accepted encodings, and send
`Vary: Accept-Encoding` when the representation varies. Streaming compression
uses the existing Writer API; no new HTTP framing or automatic negotiation is
introduced here.

## Supply and update policy

The pinned, unmodified reference C codec 1.2.1 is in
`plenty-runtime/native/brotli-1.2.1`, with its MIT license and source archive
digest. It includes the decoder block-switch validation fix for
CVE-2026-111497; the vendored source must not be replaced with an unpatched
older version. Release third-party notices include its license.

The shared build helper compiles the same C sources for runtime unit tests
and both compiler-embedded runtime archives. The explicit
`BROTLI_ENCODER_CLEANUP_ON_OOM` flag is mandatory: without it upstream selects
exit-on-allocation-failure mode. Allocation-failure tests check this behavior.
An installed Plenty compiler needs neither a system Brotli library nor the
source tree. Unused compression is removed by native section garbage collection.

This complex codec is wrapped rather than rewritten in Plenty. The Rust
`brotli` 9.1.0 allocator interface was inspected, but its encoder accesses
allocation results without a failure check (for example `RingBufferInitBuffer`).
Returning empty memory can panic; unwinding is not an allocation-error strategy
for Plenty's abort-on-panic runtime. The C interface has explicit nullable
callbacks and recoverable allocation handling.

Updates require source/license/digest and security review, plus all codec,
external-interop, malformed-input, allocation-failure, cleanup, executable-size
and installed-compiler checks. Output bytes may change with a codec update;
the interoperability and error contracts are the stable API.

## Acknowledgements

- Wraps the [Brotli](https://github.com/google/brotli) reference C codec 1.2.1, vendored in the runtime (MIT).
