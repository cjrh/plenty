# Input, output, and files

Text I/O uses strict UTF-8; [binary I/O](bytes-paths.md) preserves raw bytes.
Both use explicit [IoError](errors.md) results. Input consumption
and external writes may have partial effects before failure; errors do not promise
rollback. Error values and their `Result`/`Option` wrappers allocate nothing.
Successful writes and ordinary flushing do not promise durable storage.

- [Standard streams and arguments](streams-args.md).
- [Bytes and lossless Unix paths](bytes-paths.md).
- [Whole-file helpers](whole-files.md).
- [Owned files and modes](files.md).
- [Reading](reading.md), [writing and synchronization](writing.md), and
  [positions and truncation](positions.md).

These APIs are prelude builtins. File I/O currently supports Linux;
unsupported hosts report `IoError.Unsupported`. Standard-stream input uses the
process descriptor on Unix. Configurable encodings, public OS
descriptors, and lazy file iteration are unsupported.
Directory enumeration and file-type metadata are also unavailable; the
[finder experiment](../../examples/finder.md) records the resulting limits for
a filesystem search tool.
