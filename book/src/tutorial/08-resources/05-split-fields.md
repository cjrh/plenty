# Split text into fields

Use `text.split(separator)` to split on an explicit, nonempty string. It
returns `Result[list[str], AllocError]`:

```plenty
def main() -> Result[(), AllocError | IoError]:
    line = "Ada::Bea::::"
    parts = line.split("::")?
    print(parts)?
    print(str.repr(" / ".join(parts)?)?)?
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

This program cannot produce its fields if splitting fails, so `?` returns the
error from `main`. The same policy handles failures from joining, formatting,
and printing. Use a `match` instead when you have a useful recovery action.

An empty separator causes a runtime trap, not `AllocError`. Check
`len(separator) > 0` for user-supplied separators. Whitespace splitting without
a separator and a maximum-split argument are unsupported.
