# Format strings

An f-string is a string literal with the prefix `f`: `f"..."`, `f'...'`,
`f"""..."""`, or `f'''...'''`. Literal text uses the ordinary
[string escapes](../../facilities/01-builtins/text.md#escapes), including `\u{H...}`; `{{` and `}}` are
literal braces, and a single `}` is an error. Each `{...}` is a replacement
field:

```text
{field[=][!conversion][:spec]}
```

The expression `f"..."` has type `Result[str, std.fmt.FormatError]`, like a
collection literal's `Result`: `s = f"x={x}"?` or `print(f"x={x}"?)?`.
[`out.format(f"...")`](#writing-to-a-formatter) writes one to an existing
formatter instead. The
compiler parses the template and checks every spec while compiling. It lowers
the literal to calls on a [`std.fmt`](../../stdlib/fmt.md) string formatter,
with no format-string interpreter at run time. A module that uses an f-string
loads `std.fmt` without an `import`; naming `FormatError` in a signature still
needs `from std.fmt import FormatError`.

## Fields

A field is any expression, for example `{p.x}`, `{xs[i + 1]}`,
`{total / count:.2f}`, `{a > b}`, `{'yes' if ok else 'no'}`, or
`{parse(text)?}`. As in Python, `{a, b}` is the tuple `(a, b)`. A `:` or `!`
outside brackets ends the expression, so a lambda must be parenthesized.
Errors inside a field are reported at their position in the source.

All fields, including the nested fields of specs, are evaluated once, left to
right, before any text is built. Each value is lent to the formatting code, so
formatting moves nothing; a temporary value lives until the f-string's text is
built. A `?` in a field belongs to the enclosing function: on `Err` or
`Nothing` it returns from that function before formatting starts, exactly as
`?` in a call argument does. The `Result` of the f-string itself reports only
formatting failure, so `f"{parse(s)?}"?` has two independent `?`s.

A field cannot contain a backslash, a comment, or the quote character that
encloses the f-string. A field in a triple-quoted f-string may span lines;
a line break in its spec is a character of the spec, such as a fill.

Only the `f` prefix starts an f-string. `F"..."` and the combinations `bf`/`fb`
are errors: build text with an f-string, then convert it to bytes.

`{expr=}` writes the field's source text, including the `=` and any spaces
around it, then its value. Without a conversion or a spec the value is written
as with `!r`, so `{name=}` gives `name="Ada"`.

## Values and conversions

Without a conversion, a field is written as `print` writes it:

| Type | Spelling |
| --- | --- |
| `str` | The text itself |
| `bool` | `True` or `False` |
| Integers | Decimal |
| `f32`, `f64` | Shortest round-trip, as [`str.from`](../../facilities/01-builtins/numbers.md#formatting) |
| A class or enum with a public `display` method | That method, through `std.fmt.display` |
| Anything else, including `bytes` | The [`str.repr`](../../facilities/01-builtins/formatting.md) spelling, as `print` shows aggregates and `b"..."` values |

`!s` is the same spelling. `!r` writes `str` quoted and escaped as
`Formatter.debug_text` does, uses a type's public `debug` method when it has
one, and otherwise the `str.repr` spelling. `!a`, like Python's `ascii()`,
takes the `!r` text and spells each non-ASCII scalar as `\u{hex}` with
lowercase digits, the escape `debug_text` uses for controls:
`f"{'café'!a}"` is `"caf\u{e9}"`, which is also a valid string literal. Python spells the same escapes `\xe9`,
`\u2603`, or `\U0001f600`, with single quotes.

## Format specs

A spec follows Python's
[format specification mini-language](https://docs.python.org/3/library/string.html#formatspec):

```text
[[fill]align][sign][z][#][0][width][grouping][.precision][type]
```

- `align` is `<` (left), `>` (right), `^` (center; extra fill goes right), or
  `=` (fill between the sign and the digits, numbers only). Text defaults to
  left and numbers to right. `fill` is any single character.
- `sign` is `-` (default), `+`, or a space. `z` (floats only) drops the sign
  of a result that rounds to zero, so `{-0.0001:z.2f}` is `0.00`; `-inf` keeps
  it. `#` adds `0b`, `0o`, `0x`, or `0X` to integers, and keeps the point or
  trailing zeros of floats.
- `0` pads numbers with zeros after the sign and prefix. With grouping, the
  zeros are grouped too: `{-1234:010,}` gives `-0,001,234`. After an explicit
  alignment, `0` is the fill character.
- `width` is a minimum count of Unicode scalars; output is never truncated to
  it. `grouping` is `,` or `_`: every three integer digits, or every four with
  `_` and types `x`, `X`, `o`, and `b`.
- `precision` is the digits after the point for `e`, `f`, and `%`, the
  significant digits for `g` and for a float with no type, and the maximum
  number of scalars for text.

Width and precision are decimal numbers of at most 4096.

| Value | Types | Other limits |
| --- | --- | --- |
| Text (`str`, `bool`, values with a conversion or no built-in spelling) | none, `s` | No sign, `z`, `#`, `0`, grouping, or `=` |
| Integers | none, `d`, `x`, `X`, `o`, `b`, `n`, `c` | No precision or `z`; `,` only for decimal; `c` takes no sign, `#`, or grouping |
| `f32`, `f64` | none, `e`, `E`, `f`, `F`, `g`, `G`, `%`, `n` | |

`c` writes the Unicode scalar whose code point is the integer, laid out like a
number: right-aligned by default, and `0` pads with zeros. A value that is not
a scalar (negative, a surrogate, or above U+10FFFF) makes the f-string return
`Err(FormatError.InvalidSpec)`. `n` is Python's `n` in the C locale, which is
the locale Python formats with unless a program calls `locale.setlocale`: `d`
for integers and `g` for floats, with no grouping. Plenty has no locales, so
grouping with `n` is rejected rather than ignored.

Float types follow Python, including exponents with a sign and at least two
digits (`1.23e+03`), `%` multiplying by 100, and `g` choosing fixed or exponent
notation. Typed floats spell infinity and NaN as `inf` and `nan` (`INF`, `NAN`
for upper-case types). A float without a type or precision keeps the `print`
spelling (`1e16`, `NaN`); with a precision it behaves as Python's `.Ng`
with at least one digit after the point. Because Plenty has a `bool` type, a
spec on a `bool` formats its text rather than the number 1 or 0.

An unsupported option for the field's type is a compile error at the spec,
for example ``format spec `.2f` does not apply to i64: integers take no
precision; convert with f64(...) to format a float``. Convert explicitly to use
another type's options.

### Nested fields in a spec

A spec may contain fields, as in `{x:{width}}` or
`{x:{fill}{align}{width}.{precision}f}`. A nested field is an expression with
no `=`, conversion, spec, or further nesting. Python pastes nested values into
the spec text and parses the result at run time; Plenty instead parses the
spec while compiling, and each nested field supplies exactly one option,
chosen by its position and its type:

| Nested field | Type | Supplies |
| --- | --- | --- |
| First, before an alignment character or a second `str` field | `str` | The fill |
| First otherwise, or second after the fill | `str` | The alignment |
| Where the width's digits would be | Any integer | The width |
| After `.` | Any integer | The precision |

Any other nested field, such as an integer before the alignment or two adjacent
integer fields, is a compile error, as is `0` after a nested alignment (write
the fill instead). Everything written in the spec, including its type, is
checked against the field's type as usual; `{x:{w}.2f}` with an integer `x` is
rejected because of `.2f` whatever `w` is.

The nested values are checked when the f-string runs, and an invalid one makes
it return `Err(FormatError.InvalidSpec)` before any text is built:

- an alignment must be `""`, `<`, `>`, `^`, or `=` (`=` only for numbers);
  `""` means the default alignment;
- a fill must be one scalar, or `""` for the default space, and a non-empty
  fill needs a non-empty alignment;
- a width or precision must be in `0..=4096`.

For valid values the result is the one Python gives. Where Python would read a
pasted value differently, such as a `str` field holding `"<10"` or a fill with
an empty alignment, Plenty returns `InvalidSpec` rather than another layout.

## Writing to a formatter

`out.format(f"...")`, where `out` is a [`std.fmt.Formatter[W]`](../../stdlib/fmt.md)
or a `&mut` reference to one, writes the f-string to `out` without building a
`str`, and has type `Result[(), FormatError]`. It is the form to use in a
`display` or `debug` method:

```text
pub def display[W](self, out: &mut Formatter[W]) -> Result[(), FormatError]:
    out.format(f"({self.x}, {self.y})")
```

The argument must be an f-string literal; the compiler lowers the pair to
calls on `out`, so `out`'s byte budget, depth limit, and partial-progress rules
apply, and its scratch is reused. A field that needs a temporary string (a
spec on a value without a built-in spelling, `!r` on `str` with a spec, `!a`)
still allocates one. `format` is not an ordinary method of `Formatter`: it has
no other form, and a user type's own `format` method is unaffected.

## Cost and failure

The result of `f"..."` is built in a new string formatter whose output and
32-byte scratch are allocated for each evaluation; a float spelling longer than the scratch
grows it. A spec on a value without a built-in spelling first builds that
value's text as a temporary string. There is no byte budget: an f-string is
bounded only by allocation failure, reported as
`FormatError.Bytes(BytesError.Allocation(...))`. User `display` and `debug`
methods may nest through `std.fmt` up to 32 levels, after which the result is
`FormatError.RecursionLimit`. No partial string is returned on error.
