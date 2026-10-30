# Build text with f-strings

Put an `f` before a string to fill in values. Each `{field}` is replaced by
its value. An f-string builds a new `str`, which can fail to allocate, so it
returns a `Result` like a list literal does: write `?` after it.

```plenty
import std.fmt

def main() -> Result[(), AllocError | IoError | std.fmt.FormatError]:
    name = "Ada"
    year = 1843
    scores = [90, 85]?
    print(f"{name} published in {year}."?)?
    print(f"first score: {scores[0]}, count: {len(scores)}"?)?
    print(f"all scores: {scores}, {{braces}}"?)?
    print(f"total: {scores[0] + scores[1]}, passed: {scores[1] > 80}"?)?
    print(f"{name} was {'young' if 1843 - 1815 < 30 else 'older'}"?)?
    Ok(())
```

```output
Ada published in 1843.
first score: 90, count: 2
all scores: [90, 85], {braces}
total: 175, passed: True
Ada was young
```

A field shows its value as `print` would. Double the braces for literal ones.
A field can be any expression. A `?` inside a field returns from your
function, as `?` does anywhere else; the `?` after the f-string handles only
failure to build the text.

After a colon, a format spec chooses width, alignment, fill, sign, digit
grouping, precision, and a type, as in Python:

```plenty
import std.fmt

def main() -> Result[(), AllocError | IoError | std.fmt.FormatError]:
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

A spec can take its width or precision from a nested field, and its fill or
alignment from a nested `str` field:

```plenty
import std.fmt

def main() -> Result[(), AllocError | IoError | std.fmt.FormatError]:
    width = 8
    digits = 2
    align = "^"
    print(f"[{3.14159:{width}.{digits}f}] [{'tea':*{align}{width}}]"?)?
    Ok(())
```

```output
[    3.14] [**tea***]
```

`!r` shows the value's representation, with quotes around text, and `=` shows
the field's source before its value, which helps when debugging:

```plenty
import std.fmt

def main() -> Result[(), AllocError | IoError | std.fmt.FormatError]:
    name = "Ada"
    year = 1843
    print(f"{name!r} {name=} {year=}"?)?
    Ok(())
```

```output
"Ada" name="Ada" year=1843
```

`!a` is like `!r` but spells every non-ASCII character as an escape, so
`f"{'café'!a}"` gives `"caf\u{e9}"`, which you can paste back into source as
the string literal `"caf\u{e9}"`.

The compiler checks every spec against the field's type. A precision does not
apply to an integer:

```plenty-error
import std.fmt

def main() -> Result[(), AllocError | IoError | std.fmt.FormatError]:
    count = 3
    print(f"{count:.2f}"?)?
    Ok(())
```

```error
format spec `.2f` does not apply to i64: integers take no precision; convert with f64(...) to format a float
```

The [reference](../../design/language/04-expressions/format-strings.md) lists
every option and conversion.

An f-string suits a fixed number of pieces of any type. Its error type is
always `std.fmt.FormatError`. A function that reports only `AllocError` can
combine two strings with `a.concat(b)?` and any number of strings from a list,
set, or generator with [`separator.join(pieces)?`](../../design/facilities/01-builtins/text-building.md#joining).
