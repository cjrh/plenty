# Open a file for a block

`with open(path)? as file` makes a stream available inside the indented block.
It closes on exit, including on `return`, `?`, `break`, or `continue`. The stream
reference cannot escape the block.

`open(path)` opens an existing file for reading. `open(path, "w")` creates or
truncates a file, and `open(path, "a")` creates or appends. Each returns
`Result[File, IoError]`.
Files move on assignment and cannot be copied.

`read()` returns text from the current position through EOF, translating CRLF
and bare CR to LF and validating UTF-8. At EOF it returns an empty string.

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

The returned text owns its storage and survives closing the file. Read errors
may consume input before failing. File I/O requires Linux.
