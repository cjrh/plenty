# Reading files

All reads require exclusive access to a live readable File, even for zero limits.
They use strict UTF-8, preserve NUL, and translate LF, CRLF, and bare CR to LF.
Output is independent of the file. Buffers, long strings, and list growth are
fallible; failures may consume input but leave the file valid for another
operation or close. Invalid UTF-8 is reported after consuming input. No rollback
or concurrent-file snapshot is promised.

| Operation | Result | Behavior |
| --- | --- | --- |
| `file.read()` | `Result[str, IoError]` | Read to EOF |
| `file.read(count: i64)` | Same | Read at most count translated Unicode scalars |
| `file.readline()` | `Result[str, IoError]` | Read one line, retaining its translated terminator |
| `file.readline(count: i64)` | Same | Stop at count scalars or the first translated newline |
| `file.readlines()` | `Result[list[str], IoError]` | Eagerly collect remaining lines with translated terminators |

Negative `read` counts mean read to EOF; negative `readline` counts mean a complete
line. Zero returns empty text without consuming input or a pending LF.
Positive limits never read past a scalar and can leave a partial line for the
next call.

EOF returns `""` from `read`/`readline`. An empty line is `"\n"`; a final
unterminated line receives no added terminator. `readlines` returns an empty list
for empty input and adds no synthetic line at EOF. On failure it destroys the
collected prefix. Bounded `readline` controls memory better than eager `readlines`.

At CR, line reading returns immediately. The next read consumes an optional LF;
it does not read beyond a line merely to decide whether CR was followed by LF.
[Saved positions](positions.md) preserve this translation state. File iteration,
size hints, and configurable encodings are unsupported. To split text already in
memory using broader Unicode line boundaries, use
[splitlines](../01-builtins/text-splitting.md).
