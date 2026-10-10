# Saving and restoring text positions

`tell()` currently returns a plain `u64` used as a position cookie. Treat
its contents as opaque; this is a usage rule, not a distinct compiler-enforced type. Pass it back to `seek()` on the same
file to resume reading there, or use `seek(0u64)` to rewind. Do not calculate with
these values: they include newline state and are not byte or character counts.
Saved positions are only meaningful while the file contents remain unchanged.

```plenty
def demo() -> Result[(), IoError]:
    write_text("positions.txt", "one\r\ntwo")?
    with open("positions.txt")? as file:
        file.readline()?
        saved = file.tell()?
        print(file.read()?)?
        file.seek(saved)?
        print(file.read()?)?
    Ok(())
def main() -> Result[(), IoError]:
    demo()?
    Ok(())
```
```output
two
two
```

`truncate(size)` resizes a writable file in **bytes** and returns its new length.
With no size, it truncates at the current physical position. Neither form moves
the cursor. A text-position cookie is not a byte size. Cutting through a multibyte
character makes the file invalid UTF-8; a later read reports that error.

```plenty
def demo() -> Result[(), IoError]:
    write_text("short.txt", "keep rest")?
    with open("short.txt", "r+")? as file:
        file.read(4)?
        print(file.truncate()?)?
    print(read_text("short.txt")?)?
    Ok(())
def main() -> Result[(), IoError]:
    demo()?
    Ok(())
```
```output
4
keep
```
