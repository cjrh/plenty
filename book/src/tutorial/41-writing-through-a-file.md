# Writing through a file

`write(text)` returns a character count and preserves exact bytes, including
newlines. `flush()` flushes runtime buffers; files are currently unbuffered.
Use `sync()` when you need to request that the OS synchronize file contents and
metadata. Closing or flushing alone does not make that request.

Run this in a scratch directory: it replaces `plenty-stream.txt`.

```plenty
def write_example() -> Result[(), IoError]:
    with open("plenty-stream.txt", "w")? as file:
        print(file.write("hello\n")?)?
        file.flush()?
        file.sync()?
        file.close()?
    with open("plenty-stream.txt", "a")? as file:
        file.write("goodbye\n")?
    Ok(())

def main() -> Result[(), IoError]:
    print(write_example())?
    Ok(())
```
```output
6
Result[(), IoError].Ok(())
```

Explicit close lets you propagate its error. Context exit then closes the already
closed owner harmlessly. Writing can leave a prefix after an OS error, and append
calls are not guaranteed to be atomic records across processes. Synchronization
still follows your filesystem's durability rules.
