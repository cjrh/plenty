# Write text

The type `str` represents text. Single and double quotes both make strings;
triple quotes allow physical newlines. Escapes include `\n`, `\r`, `\t`,
escaped quotes, `\\`, and `\0` for an embedded NUL.

```plenty
def main() -> Result[(), IoError]:
    name = 'Plenty'
    print(name)?
    print(contains(name, "lent"))?
    print(len("hé"))?
    Ok(())
```

```output
Plenty
True
2
```

`print` displays text without quotes. `contains(text, part)` tests for a
substring. `len` counts Unicode scalar values, so the accented character above
counts once; this is not a byte count or a count of visible grapheme clusters.

Strings are immutable. A `mut` binding allows replacing a string, rather than
editing its bytes.
