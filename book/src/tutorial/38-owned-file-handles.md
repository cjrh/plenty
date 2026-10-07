# Owned file handles

`open(path)` opens an existing file for reading. `open(path, "w")` creates or
truncates a file, and `open(path, "a")` creates or appends. All return
`Result[File, IoError]`. Files move on assignment and cannot be copied. They
close automatically when dropped; call `close()` to observe a close error.

Run this in a scratch directory: it replaces `plenty-handle.txt`.

```plenty
def inspect_file() -> Result[(), IoError]:
    mut file = open("plenty-handle.txt", "w")?
    print(file.closed)?
    file.close()?
    print(file.closed)?
    file.close()?
    Ok(())

def main() -> Result[(), IoError]:
    print(inspect_file())?
    Ok(())
```
```output
False
True
Result[(), IoError].Ok(())
```

Closing twice succeeds. A closed file remains a valid value; it no longer owns
a handle. Automatic close cannot report errors or promise durable writes.
The initial implementation supports Linux and reports unsupported I/O elsewhere.
Use `with` to close a file at the end of a block. `read()` returns independent
text from the current position through EOF. It normalizes CRLF and bare CR to
LF and validates UTF-8. Another read at EOF returns an empty string.

Run this in a scratch directory: it replaces `plenty-reading.txt`.

```plenty
def read_example() -> Result[str, IoError]:
    write_text("plenty-reading.txt", "hello\r\nworld")?
    with open("plenty-reading.txt")? as file:
        return Ok(file.read()?)

def main() -> Result[(), IoError]:
    print(read_example())?
    Ok(())
```
```output
Result[str, IoError].Ok("hello\nworld")
```

Returning the owned text closes the file first. `with &mut file as stream:`
also closes on exit, while retaining the original owner in its closed state.
Read errors may consume input before failing. Automatic context exit cannot
report close errors; call `file.close()?` explicitly when those matter.
