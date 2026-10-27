# std.path — native paths

Import `std.path`. `Path` owns an affine `list[u8]`, preserving exact Linux path
bytes, including invalid UTF-8, spaces, tabs, CR and LF. It is not a compact byte
buffer or a Windows path abstraction. `from_bytes(bytes)` transfers the list
without allocating. `from_text(&text)` explicitly encodes UTF-8 and returns
`Result[Path, IoError]`. NUL can occur in a lexical Path; filesystem operations
reject it with `IoError.InvalidInput`.

```plenty
import std.path as path

def main() -> Result[(), AllocError | IoError]:
    base = path.from_text("data/reports")?
    child = path.from_text("2024/summary.csv")?
    full = base.join(&child)?
    print(full.display()?, full.is_absolute())?
    print(full.file_name()?.ok_or(IoError.InvalidInput)?.display()?)?
    print(full.parent()?.ok_or(IoError.InvalidInput)?.display()?)?
    print(len(full.components()?))?
    raw = path.from_bytes([110u8, 111u8, 255u8]?)
    print(raw.display()?, raw.to_text().is_err())?
    Ok(())
```
```output
data/reports/2024/summary.csv False
summary.csv
data/reports/2024
4
no\xff True
```

| Operation | Result |
| --- | --- |
| `path.as_bytes()` | Shared borrow of the existing `list[u8]`; no allocation |
| `path.to_text()` | Checked `Result[str, IoError]`; never replacement-decodes |
| `path.display()` | Escaped ASCII `Result[str, IoError]` for diagnostics |
| `path.copy()` | Detached `Result[Path, AllocError]` |
| `path.join(&child)` | `Result[Path, IoError]`; absolute child replaces the base |
| `path.is_absolute()` | Whether its Linux spelling begins with `/` |
| `path.parent()`, `path.file_name()` | `Result[Option[Path], IoError]` |
| `path.components()` | `Result[list[list[u8]], IoError]` with lexical components |

Lexical operations use Rust's Unix `Path` rules. Components remove redundant
separators and interior `.` components; an initial relative `.` is retained.
They preserve `..`, including across a symlink. Joining preserves the base's
spelling and relative root; it does not canonicalize, resolve links, test
existence, or change the working directory. A root has no parent or filename;
`a` has the empty relative path as its parent. All owned output buffers are
fallible. OS ABI details stay private.

Display is explicit: `display()` preserves printable ASCII, escapes backslash
and double quote, and emits other bytes as `\xhh`. This representation keeps
invalid UTF-8 and control bytes visible without replacement decoding; it is not
an input parser. `to_text()` validates, while `as_bytes()` allows exact binary
transport. Automatic record formatting is diagnostic list formatting, not a
round-trippable filesystem spelling. Use NUL framing with
[binary transport](../facilities/02-io/bytes-paths.md) when passing arbitrary Linux paths.

The Rust implementation baseline is `library/std/src/path.rs`; Python's
`PurePath`/`Path` separation motivates keeping lexical operations distinct from
[filesystem observations](fs/metadata.md). Broader buffers and portable path
representations remain [B12/B39 candidates](../../backlog.md); issue #76 owns
compact buffers/views. Closed issue #61 records the initial raw transport,
and issue #49 records the catalogue's decomposition rather than implementation.

## Acknowledgements

- Lexical rules follow Rust's [`std::path::Path`](https://doc.rust-lang.org/std/path/struct.Path.html); the lexical/filesystem split follows Python's [`PurePath`/`Path`](https://docs.python.org/3/library/pathlib.html).
