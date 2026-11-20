# Splitting text that is already in memory

`splitlines()` returns an independent list without line endings. Pass
`True` to keep the original endings. A final newline does not add an extra
empty line, and empty text produces an empty list. Unlike file reads, this method
also recognizes Unicode line/paragraph separators and the other Python-style
text line boundaries.

```plenty
def main() -> Result[(), AllocError | IoError]:
    print("first\r\n\nlast\n".splitlines()?)?
    print("first\r\nlast".splitlines(True)?)?
    print("".splitlines()?)?
    Ok(())
```
```output
["first", "", "last"]
["first\r\n", "last"]
[]
```

When a caller supplies an output allocator, use
[`splitlines_text`](../../design/stdlib/alloc.md#split-text-into-lines) instead.
For example, a reader can preserve the original line endings in the caller's
storage without first building an ordinary list:

```plenty
import std.alloc as alloc

def read_lines(out: &alloc.Allocator, text: &strview) -> Result[list[str], AllocError]:
    out.splitlines_text(text, keepends=True)

def main() -> Result[(), AllocError | IoError]:
    out = alloc.system()
    print(read_lines(&out, "first\r\nlast\n")?)?
    Ok(())
```
```output
["first\r\n", "last\n"]
```
