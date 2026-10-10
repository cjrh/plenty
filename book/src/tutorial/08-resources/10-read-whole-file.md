# Reading a UTF-8 file

`read_text(path)` opens, reads, and closes a file, returning
`Result[str, IoError]`. It requires Linux and strict UTF-8, translating CRLF and
bare CR to LF. Paths are relative to the process working directory. The handle
closes even when reading or allocating fails.

The `&str` parameter borrows literals and bindings implicitly;
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
