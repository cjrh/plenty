# Reading a UTF-8 file

`read_text(path)` opens, reads, and closes a file, returning
`Result[str, IoError]`. Like Python text reading, CRLF and bare CR become LF.
Encoding is always strict UTF-8. The initial file backend requires Linux.
Paths are relative to the process working directory; the helper owns and closes
its temporary handle even when reading or allocating fails.

This example creates a small configuration file, then reads it using both a
literal and a binding. The `&str` parameter borrows either argument implicitly;
`configuration(&path)` is also valid.

```plenty
def configuration(path: &str) -> Result[str, IoError]:
    read_text(path)

def main() -> Result[(), IoError]:
    write_text("configuration.txt", "mode=fast\n")?
    print(configuration("configuration.txt")?)?
    path = "configuration.txt"
    print(configuration(path)?)?
    Ok(())
```
```output
mode=fast

mode=fast

```
