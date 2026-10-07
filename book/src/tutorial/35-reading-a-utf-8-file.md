# Reading a UTF-8 file

`read_text(path)` opens, reads, and closes a file, returning
`Result[str, IoError]`. Like Python text reading, CRLF and bare CR become LF.
Encoding is always strict UTF-8. The initial file backend requires Linux.
Paths are relative to the process working directory; the helper owns and closes
its temporary handle even when reading or allocating fails.

This function can read your own configuration file. The tutorial checks its
types without depending on a file on your machine.

```plenty
def configuration(path: &str) -> Result[str, IoError]:
    read_text(path)

def main() -> Result[(), IoError]:
    print("configuration reader is ready")?
    Ok(())
```
```output
configuration reader is ready
```
