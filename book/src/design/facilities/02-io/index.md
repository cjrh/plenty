# Input, output, and files

Text I/O uses strict UTF-8; [binary I/O](bytes-paths.md) preserves raw bytes.
Both use explicit [IoError](errors.md) results. Input consumption
and external writes may have partial effects before failure; errors do not promise
rollback. Error values and their `Result`/`Option` wrappers allocate nothing.
Successful writes and ordinary flushing do not promise durable storage.

- [Standard streams and arguments](streams-args.md).
- [Bytes and lossless Unix paths](bytes-paths.md).
- [Native paths](../../stdlib/path.md), [directory enumeration](../../stdlib/fs.md),
  [metadata and links](../../stdlib/fs/metadata.md), and [walking a tree](../../stdlib/fs/walking.md).
- [Whole-file helpers](whole-files.md).
- [Owned files and modes](files.md).
- [Reading](reading.md), [writing and synchronization](writing.md), and
  [positions and truncation](positions.md).

These APIs are prelude builtins. File I/O currently supports Linux;
unsupported hosts report `IoError.Unsupported`. Standard-stream input uses the
process descriptor on Unix. Configurable encodings, public OS
descriptors, and lazy file iteration are unsupported.
The imported `std.fs`/`std.path` modules add native paths, lazy directory streams,
metadata and a Plenty walker. The [finder example](../../examples/finder.md)
searches directory roots through these APIs.
