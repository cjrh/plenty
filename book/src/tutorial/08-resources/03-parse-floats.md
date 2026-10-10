# Reading floating-point text

Floating-point parsing accepts decimal/exponent forms and surrounding whitespace.
Overflow returns `OutOfRange`; underflow can round to zero. Explicit `inf`,
`infinity`, and `nan` are accepted case-insensitively, with optional signs.

```plenty
def main() -> Result[(), Failure]:
    print(str.repr(f32.parse("1.25e2"))?)?
    print(str.repr(f32.parse("1e100"))?)?
    print(str.repr(f64.parse("-0.0"))?)?
    Ok(())
```
```output
Result[f32, ParseError].Ok(125.0)
Result[f32, ParseError].Err(ParseError.OutOfRange)
Result[f64, ParseError].Ok(-0.0)
```
