# Writing a UTF-8 file

`write_text(path, text)` creates or replaces a file and returns the number of
Unicode characters written. It writes exact UTF-8 bytes without translating
newlines. Its temporary file handle is closed before it returns. Allocation of
the path happens before truncating an existing file, but OS failures can leave
truncated or partially written contents. This is not an atomic-save operation.

Run this in a scratch directory: it replaces `plenty-example.txt`.

```plenty
def save_and_read() -> Result[str, IoError]:
    write_text("plenty-example.txt", "Hello, é!\n")?
    read_text("plenty-example.txt")

def main() -> Result[(), IoError]:
    print(save_and_read())?
    Ok(())
```
```output
Result[str, IoError].Ok("Hello, é!\n")
```

The tutorial tests execute file examples in temporary directories, independently
for compile-and-run and compiled-binary execution.
