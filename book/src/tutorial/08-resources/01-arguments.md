# Command-line arguments

`args()` returns `Result[list[str], IoError]`. Index zero is the executable's
invocation name, followed by the supplied arguments. Pass arguments to a compiled
binary, for example `./program first "two words"`; quoted spaces remain within
one argument. Each call creates an independent list and reports allocation or
UTF-8 errors.

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
