# Open a file for a block

Use `with` to keep a file open for several operations. The expression after
`with` acquires the manager; `as file` names the stream inside the indented block.
The file closes when the block ends, including on `return`, `?`, `break`, or
`continue`. The stream reference cannot escape the block.

`open(path)` opens an existing file for reading. `open(path, "w")` creates or
truncates a file, and `open(path, "a")` creates or appends. Each returns
`Result[File, IoError]`, so acquisition uses `?` before entering the block.
Files move on assignment and cannot be copied.

Run this in a scratch directory: it replaces `plenty-reading.txt`.

```plenty
def read_example() -> Result[str, IoError]:
    write_text("plenty-reading.txt", "hello\r\nworld")?
    with open("plenty-reading.txt")? as file:
        return Ok(file.read()?)

def main() -> Result[(), IoError]:
    print(read_example()?)?
    Ok(())
```
```output
hello
world
```

Returning the owned text closes the file first. `with &mut file as stream:`
also closes on exit, while retaining the original owner in its closed state.
Read errors may consume input before failing. Automatic context exit cannot
report close errors; call `file.close()?` explicitly when those matter.

`read()` returns independent text from the current position through EOF. It
normalizes CRLF and bare CR to LF and validates UTF-8. Another read at EOF
returns an empty string. File I/O currently requires Linux.
