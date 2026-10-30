# Write text

The type `str` represents text. Single and double quotes both make strings;
triple quotes allow physical newlines. Escapes include `\n`, `\r`, `\t`,
escaped quotes, `\\`, `\0` for an embedded NUL, and `\u{H...}` for any
character by its hexadecimal code point, such as `"e\u{301}"` for an `e`
followed by a combining accent. See [escapes](../../design/facilities/01-builtins/text.md#escapes).

```plenty
def main() -> Result[(), IoError]:
    name = 'Plenty'
    print(name)?
    print(contains(name, "lent"))?
    print(len("hé"))?
    print("hé".byte_len())?
    Ok(())
```

```output
Plenty
True
2
3
```

`print` displays text without quotes. `contains(text, part)` tests for a
substring. `len` counts Unicode scalar values, so the accented character above
counts once; this is not a count of visible grapheme clusters. `byte_len()`
gives the size of the UTF-8 encoding instead, which is what a file, a buffer,
or a size limit measures.

Strings are immutable. A `mut` binding allows replacing a string, rather than
editing its bytes.

Compare strings in Unicode scalar order, inspect ASCII characters, or convert
a single character to its code point:

```plenty
def main() -> Result[(), AllocError | IoError | ParseError]:
    print("apple" < "pear")?
    print("abc".isalpha())?
    print("123".isdigit())?
    print(ord("é")?)?
    print(chr(233)?)?
    print("Hello".lower()?)?
    print("Straße".upper()?)?
    print("  one\t two\n".split()?)?
    Ok(())
```

```output
True
True
True
233
é
hello
STRASSE
["one", "two"]
```

`ord` requires exactly one Unicode scalar; `chr` takes a `u32` code point and
rejects surrogates and values outside Unicode. Both return `Result` and allocate
nothing. Case conversion and splitting return fallible, independently owned
results. The ASCII predicates return false for empty or non-ASCII text.
