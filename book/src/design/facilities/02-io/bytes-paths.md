# Bytes and lossless Unix paths

Raw transport uses owned `list[u8]` values. These are ordinary mutable lists,
with checked indexing, borrowing, and fallible growth; the current representation
uses general list slots rather than a compact byte buffer. A Unix path is a byte
list used without decoding. Filesystem calls reject embedded NUL and otherwise
preserve every byte, including CR, LF, tabs, and invalid UTF-8.

All operations below are prelude builtins. Arguments are observed in source
order and remain available. Results use `IoError`; allocation failures are
`IoError.Data(DataError.Allocation(...))`.

| Operation | Result | Contract |
| --- | --- | --- |
| `encode_utf8(text)` | `Result[list[u8], IoError]` | Independent owned copy of the exact UTF-8 bytes |
| `decode_utf8(bytes)` | `Result[str, IoError]` | Strict validation and independent text; invalid input returns `DataError.InvalidUtf8` |
| `args_bytes()` | `Result[list[list[u8]], IoError]` | Independent snapshot including invocation name at index zero; no decoding |
| `input_bytes(delimiter: u8)` | `Result[Option[list[u8]], IoError]` | Read one stdin record, removing only its delimiter |
| `write_stdout_bytes(bytes)` | `Result[i64, IoError]` | Exact bytes with no added framing; returns byte count |
| `write_stderr_bytes(bytes)` | `Result[i64, IoError]` | Same contract on stderr |
| `read_bytes(path)` | `Result[list[u8], IoError]` | Read whole file without decoding or newline translation |
| `write_bytes(path, bytes)` | `Result[i64, IoError]` | Create or truncate, write exact bytes, check close, return byte count |

`input_bytes(0u8)` provides NUL framing for Unix paths, whose names cannot contain
NUL. `Some(empty)` represents an empty record; `Nothing` means EOF before any
record byte. A final unterminated nonempty record is accepted. CR and LF have no
special meaning. Input reads no bytes beyond the delimiter and retries interrupted
reads. Allocation or I/O failure can consume part of a record; no rollback or
resynchronization is promised. Construction may fail even at EOF.

Writers use bounded stack chunks without allocating an output buffer. They retry
interrupted writes and complete all bytes on success, but errors can follow a
written prefix. Use the existing `flush_stdout()` or `flush_stderr()` to report
deferred output errors. Calls and NUL records are not atomic against concurrent
writers.

Binary file helpers use the same Linux open/creation/symlink rules as
[text helpers](whole-files.md). Path conversion is fallible and precedes opening,
so allocation failure cannot truncate a destination. No output allocation follows
opening. Reads release partial buffers and descriptors on error; successful file
operations check close errors. Success does not imply atomic replacement or durable
storage. File helpers on other hosts return `IoError.Unsupported`; raw stdin uses
the Unix process descriptor.

For display, decode explicitly or format the byte list with `str.repr`; keep the
original bytes for filesystem operations. Replacement decoding is never used.
The imported [std.path](../../stdlib/path.md) module provides Linux Path owners over this
transport, and [std.fs](../../stdlib/fs.md) provides directory enumeration and
metadata. Compact buffers, byte views, portable path representations, binary
File methods and reader/writer objects remain within issue #49's broader scope.
The [finder example](../../examples/finder.md) exercises raw arguments, matching,
and NUL input/output against real Unix filenames.
