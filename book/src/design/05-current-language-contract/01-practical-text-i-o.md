# Practical text I/O

`write_stdout(text) -> Result[i64, IoError]` borrows UTF-8 text, writes it without
adding a newline, and returns its Unicode-scalar count. `IoError.System(i32)`
preserves a native OS error code and is produced only when the operating system
supplied one. `IoError.Data(DataError.InvalidUtf8)` reports invalid encoding, and
`IoError.Data(DataError.Allocation(AllocError))` reports recoverable allocation
failure. Failures detected without an OS code are payload-free variants:

| Variant | Meaning |
|---|---|
| `InvalidMode` | The mode passed to `open` is not one of its modes |
| `Closed` | The file was already closed |
| `NotReadable` | The file was not opened for reading |
| `NotWritable` | The file was not opened for writing |
| `InvalidInput` | An argument is outside the accepted range, such as a path containing NUL or a negative size |
| `Unsupported` | The host does not support the operation |
| `Other` | Another failure for which the operating system supplied no code |

These error values and their Result wrappers all have inline layouts, and
constructing one allocates nothing.
Writes may have visible partial effects before returning an error. Successful
writes may still be buffered; neither writing nor ordinary flushing implies
durable disk storage. `print` also returns `Result[(), IoError]`.

`write_stderr(text)` has the same contract for the diagnostic stream.
`flush_stdout()` and `flush_stderr()` return `Result[(), IoError]`, exposing
deferred buffered-write failures. Standard stream handles remain process-owned.

`input() -> Result[Option[str], IoError]` reads one stdin line, removing LF or
CRLF, preserving other characters, and distinguishing an empty line from EOF.
Invalid UTF-8 is an error. Buffer growth and result allocation are recoverable;
failure may consume a prefix or the complete line. It never reads ahead into
the next line. The current Unix backend reads the process descriptor directly;
other hosts report unsupported I/O. Prompt arguments, universal bare-CR newline
translation, binary input, and buffered stream objects are deferred.

`args() -> Result[list[str], IoError]` snapshots the executable's arguments,
including its invocation name at index zero. Every call creates an independent
list with fallibly allocated strings. OS-provided argument storage is borrowed
only during construction; invalid UTF-8 is rejected, never replaced or escaped.
The native startup retains the process argument pointers without allocating.

`read_text(path) -> Result[str, IoError]` reads and closes a whole UTF-8 file,
normalizing CRLF and bare CR to LF as Python text readers do. NUL characters in
contents are preserved; NUL in paths is rejected. Decoding is strict, with no
locale dependence or BOM removal. Path conversion, buffer growth, and the final
string are fallible; errors reclaim partial buffers and close the descriptor.
The initial file backend uses Linux open/read/close through Rust-owned handles,
with no public ABI exposure; other hosts report unsupported I/O. Paths are
relative to the process working directory unless absolute. Ordinary symlink
resolution applies. Reads do not promise a snapshot against concurrent writers.

`write_text(path, text) -> Result[i64, IoError]` creates or truncates a file,
writes the exact UTF-8 bytes, closes it, and returns the Unicode-scalar count.
Path allocation precedes opening/truncation; no output allocation follows it.
The existing file can be truncated or partially written on an OS failure.
Successful writes check close errors but do not imply `fsync` durability or
atomic replacement. Creation uses permissions `0666` restricted by the process
umask. Arguments are observed once in source order and remain usable.
Tutorial executions use separate temporary working directories for each mode.

`append_text(path, text)` has the same result and byte/cleanup rules as
`write_text`, but opens with append semantics and never truncates existing data.
It creates a missing file, including for empty text. Each OS write appends at the
current end; a logical call may require multiple writes and is not an atomic
record against concurrent writers. Allocation fails before opening; OS failures
can leave an appended prefix. Owned File streams are described below.

These initial functions are explicit prelude builtins. Future standard-library
modules and stream methods can build on their error, encoding, and resource
contracts; no file object or OS descriptor is exposed as a language value yet.
