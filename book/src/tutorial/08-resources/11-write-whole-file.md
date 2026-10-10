# Writing a UTF-8 file

`write_text(path, text)` creates or replaces a file and returns the number of
Unicode characters written. It writes exact UTF-8 bytes without translating
newlines and closes the handle before returning. OS failures can leave truncated
or partially written contents; this is not an atomic save.

Run this in a scratch directory: it replaces `plenty-example.txt`.

```plenty
def save_and_read() -> Result[str, IoError]:
    write_text("plenty-example.txt", "Hello, é!\n")?
    read_text("plenty-example.txt")

def main() -> Result[(), Failure]:
    print(str.repr(save_and_read())?)?
    Ok(())
```
```output
Result[str, IoError].Ok("Hello, é!\n")
```
