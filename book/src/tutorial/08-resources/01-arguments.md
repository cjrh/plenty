# Command-line arguments

`args()` returns `Result[list[str], IoError]`. Index zero is the executable's
invocation name, followed by the supplied arguments. Spaces within one argument
remain intact. Each call produces an independent list; allocation failure and
invalid UTF-8 are recoverable errors. Pass arguments when launching a compiled
binary, for example `./program first "two words"`.

```plenty
def count_arguments() -> Result[bool, IoError]:
    values = args()?
    Ok(len(values) >= 1)

def main() -> Result[(), Failure]:
    print(count_arguments()?)?
    Ok(())
```
```output
True
```
