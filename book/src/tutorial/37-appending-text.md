# Appending text

`append_text(path, text)` adds exact UTF-8 bytes to the end of a file, creating
it if necessary. It returns the number of characters appended. Like writing,
it can leave partial output after an OS error; one call is not guaranteed to be
an atomic record when several processes write concurrently.

Run this in a scratch directory: it replaces `plenty-log.txt` before appending.

```plenty
def log_example() -> Result[str, IoError]:
    write_text("plenty-log.txt", "started\n")?
    append_text("plenty-log.txt", "finished\n")?
    read_text("plenty-log.txt")

def main() -> Result[(), Failure]:
    print(str.repr(log_example())?)?
    Ok(())
```
```output
Result[str, IoError].Ok("started\nfinished\n")
```

These helpers cover whole files. Automatic cleanup closes their private handles
on both success and failure.
