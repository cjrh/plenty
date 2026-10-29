# std.cli.fileinput

Read text lines from successive files or standard input without loading whole
files. The module is implemented in Plenty over [`std.io`](io.md), with owned
[fallible generators](../language/09-iteration/03-generators.md), not global state.

## Recommended loop

`lines()` snapshots native command-line arguments after the executable name.
With no arguments it reads stdin. Use `line?` inside the loop: the outer `?`
handles argument construction, while each item can report opening, reading,
decoding, or allocation failure.

```plenty
import std.cli.fileinput as fileinput
from std.cli.fileinput import FileInputError

def main() -> Result[(), IoError | AllocError | FileInputError]:
    for line in fileinput.lines()?:
        write_stdout(line?)?
    flush_stdout()?
    Ok(())
```
```output
```

Compile this filter and run `./filter first.txt second.txt`, or pipe input into
`./filter`. `write_stdout` does not add a second newline. `print` would.

## Explicit paths and options

| API | Result | Behavior |
|---|---|---|
| `lines()` | `Result[Generator[Result[str, FileInputError]], IoError \| AllocError]` | Raw argv after the executable name; **no option parsing** |
| `files(paths: list[Path])` | `Generator[Result[str, FileInputError]]` | Consumes explicit native paths in order; an empty list selects stdin |

Applications with flags should parse them with [`std.cli`](cli.md), collect the
selected `Path` values, and call `files`. The module never treats application
flags specially: `lines()` attempts to open even `--help` as a filename.
`Path` preserves native bytes, including non-UTF-8 filenames. Only an exact
one-byte `-` selects stdin; use `./-` for a file literally named `-`.

```plenty
import std.cli.fileinput as fileinput
import std.path as paths
from std.cli.fileinput import FileInputError

def main() -> Result[(), IoError | AllocError | FileInputError]:
    write_text("first.txt", "one\ntwo")?
    write_text("second.txt", "three\n")?
    names = [paths.from_text("first.txt")?, paths.from_text("second.txt")?]?
    for line in fileinput.files(names):
        print(str.repr(line?)?)?
    Ok(())
```
```output
"one\n"
"two"
"three\n"
```

## Line, buffering, and ownership contracts

- Lines end at LF, retaining that LF and any preceding CR. Lone CR is ordinary
  text. The last nonempty, unterminated line is returned without adding LF.
  No line is synthesized at EOF, and empty files yield nothing.
- Decoding is strict UTF-8, including stdin. A completed line is validated as a
  whole, so multibyte characters may cross read-buffer boundaries. Invalid
  input is an error, not replacement text. There is no encoding option.
- Files open lazily, one at a time, using ordinary path and symlink semantics.
  Each file and each stdin occurrence has an **8192-byte `BufReader`**. Unread
  bytes survive yields. Delimiter scanning advances through each chunk once;
  line accumulation uses amortized buffer growth, then one text conversion.
  Storage is the owned path list, a fixed read buffer, and the current line
  (including temporary bytes during conversion), not the entire file. A single
  very long line can still exhaust memory; there is no line-length limit.
- Each `-` reads the same process stream from its current position. After EOF,
  another `-` normally yields nothing; it can read new data if the underlying
  stream has subsequently been reset or an interactive source resumes.
- Exhaustion explicitly closes each owned file and reports a close error.
  `break`, return, propagation, or dropping the generator closes any remaining
  file through ordinary ownership cleanup; close errors during destruction
  cannot be reported. Stdin is **never closed**.
- Buffering reads ahead. Do not mix an active iterator with other stdin readers.
  Dropping it early discards its unread buffered bytes; a fresh iterator cannot
  recover those bytes. This applies even though descriptor 0 stays open.

## Errors

`FileInputError` has `Open(Option[Path], StreamError)`,
`Read(Option[Path], StreamError)`, `Text(Option[Path], BytesError)`, and
`Close(Option[Path], StreamError)` variants. The path is the exact owned native
path supplied by the caller (`Some` even for `-`); `Nothing` identifies implicit
stdin. `Open` includes allocating the read buffer. `Text` includes allocating
line storage and strict UTF-8 conversion; line growth during reading can also
return `Read(..., StreamError.Allocation(...))`.

Underlying errors are preserved, including OS codes and allocation causes.
An error yields **one `Err` and terminates the whole sequence**, not just that
file. Owned file resources are released before yielding a read or text error.
Already returned lines remain valid; failed partial lines are not returned and
consumed input cannot be rolled back. Handle or propagate each item's `Result`;
a loop does not propagate it automatically.

This is core input only: no rewriting, backups, open hooks, filename/line-number
tracking, skip-file operation, or shared global iterator. Unlike Python's default
text mode, it does not perform universal-newline translation.

## Acknowledgements

- [Python `fileinput`](https://docs.python.org/3/library/fileinput.html) models the
  ordered-files, stdin fallback, dash sentinel, and retained-line-ending API.
  This is an original Plenty implementation, not copied Python code.
