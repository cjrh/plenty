# Splitting text that is already in memory

`splitlines()` returns an independent list without line endings. Pass
`True` to keep the original endings. A final newline does not add an extra
empty line, and empty text produces an empty list. Unlike file reads, this method
also recognizes Unicode line/paragraph separators and the other Python-style
text line boundaries.

```plenty
def main() -> Result[(), Failure]:
    print("first\r\n\nlast\n".splitlines()?)?
    print("first\r\nlast".splitlines(True)?)?
    print("".splitlines()?)?
    Ok(())
```
```output
["first", "", "last"]
["first\r\n", "last"]
[]
```
