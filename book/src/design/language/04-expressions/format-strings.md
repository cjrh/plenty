# Format strings

An f-string is a string literal with the prefix `f`: `f"..."`, `f'...'`,
`f"""..."""`, or `f'''...'''`. Literal text uses the ordinary
[string escapes](../../facilities/01-builtins/text.md); `{{` and `}}` are
literal braces, and a single `}` is an error. Each `{...}` is a replacement
field:

```text
{field[=][!conversion][:spec]}
```

The expression `f"..."` has type `Result[str, std.fmt.FormatError]`, like a
collection literal's `Result`: `s = f"x={x}"?` or `print(f"x={x}"?)?`. The
compiler parses the template and checks every spec while compiling. It lowers
the literal to calls on a [`std.fmt`](../../stdlib/fmt.md) string formatter,
with no format-string interpreter at run time. A module that uses an f-string
loads `std.fmt` without an `import`; naming `FormatError` in a signature still
needs `from std.fmt import FormatError`.

## Fields

A field is a name or literal followed by any number of field accesses, index
operations, and calls, for example `{p.x}`, `{xs[0]}`, `{d['k']}`,
`{len(xs)}`, or `{account.balance()}`. Call arguments and indices may also be
literals, negative number literals, borrows (`&x`, `&mut x`) of such
expressions, and named arguments. Other expressions, including arithmetic,
conditionals, and `?`, are rejected; bind them to a name first. Fields are
evaluated once, left to right, and each is lent to the formatting code, so
formatting moves nothing. A field cannot contain a backslash, a comment, or
the quote character that encloses the f-string. A field in a triple-quoted
f-string may span lines.

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
one, and otherwise the `str.repr` spelling. `!a` is not supported.

## Format specs

A spec follows Python's
[format specification mini-language](https://docs.python.org/3/library/string.html#formatspec):

```text
[[fill]align][sign][#][0][width][grouping][.precision][type]
```

- `align` is `<` (left), `>` (right), `^` (center; extra fill goes right), or
  `=` (fill between the sign and the digits, numbers only). Text defaults to
  left and numbers to right. `fill` is any single character.
- `sign` is `-` (default), `+`, or a space. `#` adds `0b`, `0o`, `0x`, or `0X`
  to integers, and keeps the point or trailing zeros of floats.
- `0` pads numbers with zeros after the sign and prefix. With grouping, the
  zeros are grouped too: `{-1234:010,}` gives `-0,001,234`. After an explicit
  alignment, `0` is the fill character.
- `width` is a minimum count of Unicode scalars; output is never truncated to
  it. `grouping` is `,` or `_`: every three integer digits, or every four with
  `_` and types `x`, `X`, `o`, and `b`.
- `precision` is the digits after the point for `e`, `f`, and `%`, the
  significant digits for `g` and for a float with no type, and the maximum
  number of scalars for text.

Width and precision are decimal numbers of at most 4096. Nested fields such
as `{x:{width}}` are not supported.

| Value | Types | Other limits |
| --- | --- | --- |
| Text (`str`, `bool`, values with a conversion or no built-in spelling) | none, `s` | No sign, `#`, `0`, grouping, or `=` |
| Integers | none, `d`, `x`, `X`, `o`, `b` | No precision; `,` only for decimal |
| `f32`, `f64` | none, `e`, `E`, `f`, `F`, `g`, `G`, `%` | |

Float types follow Python, including exponents with a sign and at least two
digits (`1.23e+03`), `%` multiplying by 100, and `g` choosing fixed or exponent
notation. Typed floats spell infinity and NaN as `inf` and `nan` (`INF`, `NAN`
for upper-case types). A float without a type or precision keeps the `print`
spelling (`1e16`, `NaN`); with a precision it behaves as Python's `.Ng`
with at least one digit after the point. `n`, `c`, and the `z` option are not
supported. Because Plenty has a `bool` type, a spec on a `bool` formats its
text rather than the number 1 or 0.

An unsupported option for the field's type is a compile error at the spec,
for example ``format spec `.2f` does not apply to i64: integers take no
precision; convert with f64(...) to format a float``. Convert explicitly to use
another type's options.

## Cost and failure

The result is built in a new string formatter whose output and 32-byte scratch
are allocated for each evaluation; a float spelling longer than the scratch
grows it. A spec on a value without a built-in spelling first builds that
value's text as a temporary string. There is no byte budget: an f-string is
bounded only by allocation failure, reported as
`FormatError.Bytes(BytesError.Allocation(...))`. User `display` and `debug`
methods may nest through `std.fmt` up to 32 levels, after which the result is
`FormatError.RecursionLimit`. No partial string is returned on error.
