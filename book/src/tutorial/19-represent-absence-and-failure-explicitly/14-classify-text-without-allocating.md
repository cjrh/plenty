# Classify text without allocating

`isascii()` accepts only ASCII characters, including controls and NUL.
`isspace()` checks for nonempty text containing only Unicode whitespace:

```plenty
def main() -> Result[(), IoError]:
    print("hello".isascii())?
    print("é".isascii())?
    print("".isascii())?
    print(" \t\n".isspace())?
    print(" x ".isspace())?
    print("".isspace())?
    Ok(())
```

```output
True
False
True
True
False
False
```

Both queries borrow their receiver and allocate nothing. Whitespace means the
same Unicode White_Space characters removed by `strip`; NUL and zero-width
space do not count.
