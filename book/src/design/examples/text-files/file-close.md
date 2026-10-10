# Observe file closing

Files close automatically when dropped. Call `close()` explicitly to observe
a close error. A closed file remains a valid value without an open handle.

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
    inspect_file()?
    Ok(())
```
```output
False
True
```

Closing twice succeeds. Automatic close cannot report errors or promise durable
writes.
