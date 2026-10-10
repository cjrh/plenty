# Split text into fields

Use `text.split(separator)` to split on an explicit, nonempty string. It
returns `Result[list[str], AllocError]`:

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
occur produces a single piece containing the whole input.

Each piece owns independent storage. Splitting borrows both inputs and leaves
them unchanged, including on allocation failure.

An empty separator causes a runtime trap, not `AllocError`. Check
`len(separator) > 0` for user-supplied separators. Whitespace splitting without
a separator and a maximum-split argument are unsupported.
