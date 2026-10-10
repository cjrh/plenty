# Split text into fields

Use `text.split(separator)` to split on an explicit, nonempty string. It
returns `Result[list[str], AllocError]`, so `?` can propagate allocation failure:

```plenty
def fields(line: &str) -> Result[list[str], AllocError]:
    line.split("::")

def main() -> Result[(), Failure]:
    line = "Ada::Bea::::"
    match fields(&line):
        case Ok(parts):
            print(parts)?
            print(str.repr(" / ".join(parts)?)?)?
        case Err(error):
            print(error)?
    print(line)?
    print("".split(",")?)?
    Ok(())
```
```output
["Ada", "Bea", "", ""]
"Ada / Bea /  / "
Ada::Bea::::
[""]
```

Adjacent and trailing separators preserve empty fields. Matches do not overlap:
`"aaaaa".split("aa")` succeeds with `["", "", "a"]`. A separator that does not
occur produces a single piece containing the whole input. Unicode and embedded
`\0` work in both the input and the separator.

The operation observes both strings. Each output piece has independent storage;
the result remains valid after the input is dropped. If allocation fails partway
through, the partial result is cleaned up and both inputs remain unchanged.

An empty separator is invalid and terminates the program with a runtime error;
it does not return `AllocError`. Check `len(separator) > 0` when the separator
comes from user input. There is currently no omitted-separator whitespace mode
or maximum-split argument.
