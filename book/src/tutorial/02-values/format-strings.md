# Build text with f-strings

Put an `f` before a string to fill in values. Each `{field}` is replaced by
its value. An f-string builds a new `str`, which can fail to allocate, so it
returns a `Result` like a list literal does: write `?` after it.

```plenty
def main() -> Result[(), Failure]:
    name = "Ada"
    year = 1843
    scores = [90, 85]?
    print(f"{name} published in {year}."?)?
    print(f"first score: {scores[0]}, count: {len(scores)}"?)?
    print(f"all scores: {scores}, {{braces}}"?)?
    Ok(())
```

```output
Ada published in 1843.
first score: 90, count: 2
all scores: [90, 85], {braces}
```

A field shows its value as `print` would. Double the braces for literal ones.
A field may be a name or a literal with field access, indexing, and calls.
Bind anything else, such as arithmetic, to a name first.

After a colon, a format spec chooses width, alignment, fill, sign, digit
grouping, precision, and a type, as in Python:

```plenty
def main() -> Result[(), Failure]:
    item = "tea"
    price = 3.14159
    count = 1234567
    print(f"[{item:<6}] [{item:>6}] [{item:*^7}]"?)?
    print(f"{price:.2f} {price:8.3f} {price:+.1e} {0.25:.0%}"?)?
    print(f"{count:,} {count:_} {255:#x} {255:08b} {-42:05}"?)?
    Ok(())
```

```output
[tea   ] [   tea] [**tea**]
3.14    3.142 +3.1e+00 25%
1,234,567 1_234_567 0xff 11111111 -0042
```

`!r` shows the value's representation, with quotes around text, and `=` shows
the field's source before its value, which helps when debugging:

```plenty
def main() -> Result[(), Failure]:
    name = "Ada"
    year = 1843
    print(f"{name!r} {name=} {year=}"?)?
    Ok(())
```

```output
"Ada" name="Ada" year=1843
```

The compiler checks every spec against the field's type. A precision does not
apply to an integer:

```plenty-error
def main() -> Result[(), Failure]:
    count = 3
    print(f"{count:.2f}"?)?
    Ok(())
```

```error
format spec `.2f` does not apply to i64: integers take no precision; convert with f64(...) to format a float
```

The [reference](../../design/language/04-expressions/format-strings.md) lists
every option and conversion.
