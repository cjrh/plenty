# Standard streams and arguments

## Output

`write_stdout(text: str) -> Result[i64, IoError]` and `write_stderr(text)`
observe text and write its exact UTF-8 bytes without adding a newline. Success
returns the Unicode-scalar count. Errors can follow partial output.
Stderr writes go straight to descriptor 2, so a closed or read-only stderr is
`IoError.System(9)` (`EBADF`). Stdout writes are buffered by the Rust standard
library, which reports `EBADF` as success; a closed or read-only stdout
discards output without an error.

`print(values...) -> Result[(), IoError]` formats supported values separated by
spaces and adds a newline; no arguments produce an empty line;
see [formatting](../01-builtins/formatting.md) for allocation and Result restrictions.
`flush_stdout()` and `flush_stderr()` return `Result[(), IoError]`, exposing
deferred buffered-write errors. These operations retain the process-owned stream
handles. Success and flushing do not imply durable storage.

## Input

`input() -> Result[Option[str], IoError]` reads one stdin line, removing LF or
CRLF and preserving other characters. `Some("")` is an empty line; `Nothing`
is EOF. Decoding is strict UTF-8.

Buffer growth and result construction are recoverable. Failure may consume a
prefix or complete line; the reader does not read ahead into the next line.
The current Unix backend reads the process descriptor directly. Prompt arguments,
bare-CR newline translation and buffered stream objects are
unsupported.
Use [input_bytes](bytes-paths.md) for binary records without newline conversion.

## Arguments

`args() -> Result[list[str], IoError]` snapshots all executable arguments,
including the invocation name at index zero. Each call produces an independent
list with fallible text construction. Invalid UTF-8 is rejected rather than
replaced or escaped; failed construction releases partial output. OS argument
storage is only borrowed during construction.
Use [args_bytes](bytes-paths.md) for lossless Unix arguments and paths.
