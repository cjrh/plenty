# Input, output, and files

I/O uses strict UTF-8 and explicit [IoError](errors.md) results. Input consumption
and external writes may have partial effects before failure; errors do not promise
rollback. Error values and their `Result`/`Option` wrappers allocate nothing.
Successful writes and ordinary flushing do not promise durable storage.

- [Standard streams and arguments](streams-args.md).
- [Whole-file helpers](whole-files.md).
- [Owned files and modes](files.md).
- [Reading](reading.md), [writing and synchronization](writing.md), and
  [positions and truncation](positions.md).

These APIs are prelude builtins. File I/O currently supports Linux;
unsupported hosts report `IoError.Unsupported`. Standard-stream input uses the
process descriptor on Unix. Binary I/O, configurable encodings, public OS
descriptors, and lazy file iteration are unsupported.
