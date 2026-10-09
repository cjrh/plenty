# 10. Work with text

Both single and double quotes make strings. Triple-quoted strings can contain
physical newlines. Common escapes include `\n`, `\r`, `\t`, escaped quotes,
and `\\`. Strings support UTF-8 and embedded NUL using `\0`.

```plenty
def main() -> Result[(), Failure]:
    message = ('Hello, ' + "Plenty!")?
    print(message)?
    print(contains(message, "Plenty"))?
    Ok(())
```

```output
Hello, Plenty!
True
```

`print` accepts one value and adds a newline. It prints text without quotes and
integers without width suffixes. `contains(text, part)` tests for a substring.
There is just one string type, `str`. Strings are immutable values; `mut` permits
replacing a binding rather than editing its bytes. `len` counts Unicode scalar
values; indexing returns one scalar as a `str`, and iteration yields one per
step. Neither allocates: strings of up to seven UTF-8 bytes, which includes every
single character, are stored inline in the value. Storage uses explicit lengths,
so an embedded NUL does not end a string. Interpolation and general conversion
to strings are not available yet.

```plenty
def main() -> Result[(), Failure]:
    text = "é\0😀"
    print(len(text))?
    print(text[-1])?
    print("\0" in text)?
    print([text]?)?
    Ok(())
```

```output
3
😀
True
["é\0😀"]
```
