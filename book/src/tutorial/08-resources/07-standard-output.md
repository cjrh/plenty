# Writing text with recoverable errors

`write_stdout(text)` borrows a string and writes exactly its contents, without a
newline. Success returns the number of Unicode characters. An I/O error can
follow a partial write, so retrying the whole string can duplicate output.

```plenty
def main() -> Result[(), AllocError | IoError]:
    result = write_stdout("Hello!\n")
    print(result?)?
    Ok(())
```
```output
Hello!
7
```

`IoError.System(code)` carries a native OS error code. Decoding, allocation, and
stream-state errors have separate variants; the
[text I/O reference](../../design/facilities/02-io/index.md)
lists them all.
