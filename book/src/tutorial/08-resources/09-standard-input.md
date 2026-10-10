# Reading a line

`input()` returns `Result[Option[str], IoError]`. It removes a trailing LF or
CRLF. `Some("")` means an empty line; `Nothing` means end-of-file. UTF-8 errors
and allocation failures return `Err`. A failure can consume input, so retrying
does not promise to repeat the same line. Input currently requires a Unix host.
For a prompt, call `write_stdout` and `flush_stdout` before `input()`.

Call `echo_line()?` repeatedly until it returns `False` to process stdin. This
example's `main` does not read input.

```plenty
def echo_line() -> Result[bool, IoError]:
    match input()?:
        case Some(line):
            write_stdout(line)?
            write_stdout("\n")?
            Ok(True)
        case Nothing:
            Ok(False)

def main() -> Result[(), IoError]:
    print("echo_line is ready")?
    Ok(())
```
```output
echo_line is ready
```
