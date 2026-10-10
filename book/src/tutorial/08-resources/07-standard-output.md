# Writing text with recoverable errors

`write_stdout(text)` borrows a string and writes exactly its contents, without a
newline. Success returns the number of Unicode characters. An I/O error can
follow a partial write, so retrying the whole string can duplicate output.

```plenty
def main() -> Result[(), Failure]:
    result = write_stdout("Hello!\n")
    print(result?)?
    Ok(())
```
```output
Hello!
7
```

`IoError.System(code)` carries a native OS error code. `IoError.Data` carries
`DataError.InvalidUtf8` or `DataError.Allocation(AllocError)`. Failures that have
no OS code are separate variants, such as `IoError.Closed` for a file that was
already closed and `IoError.NotWritable` for a file opened only for reading; the
[text I/O reference](../../design/05-current-language-contract/01-practical-text-i-o.md)
lists them all. All these error values allocate nothing.
