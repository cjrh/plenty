# Write through a file

Use a writable stream when several writes belong to one open file. `write(text)`
returns the number of Unicode characters written and preserves exact UTF-8 bytes,
including newlines. Mode `"w"` replaces a file; `"a"` appends to it.

Run this in a scratch directory: it replaces `plenty-stream.txt`.

```plenty
def main() -> Result[(), IoError]:
    with open("plenty-stream.txt", "w")? as file:
        print(file.write("hello\n")?)?
        file.write("world\n")?
        file.close()?
    with open("plenty-stream.txt", "a")? as file:
        file.write("goodbye\n")?
    print(read_text("plenty-stream.txt")?)?
    Ok(())
```
```output
6
hello
world
goodbye

```

Explicit `close()?` lets the function report a close error. The block's automatic
exit can then close the already closed owner harmlessly. Automatic cleanup
cannot return a close error. A failed write can leave a prefix; retrying the whole
string may duplicate output. Appends are not guaranteed to be atomic records
across processes.

For flushing, disk synchronization, and other detailed contracts, see the
[text I/O reference](../../design/05-current-language-contract/01-practical-text-i-o.md).
