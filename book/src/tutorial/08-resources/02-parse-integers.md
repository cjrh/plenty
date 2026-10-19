# Reading integers from text

Use a sized type's `parse` method for external text. It borrows the string and
returns an allocation-free `Result`. Decimal ASCII digits and an optional sign
are accepted, with surrounding Unicode whitespace ignored. Numeric prefixes,
underscores, and non-ASCII digits are not accepted.

```plenty
def main() -> Result[(), AllocError | IoError]:
    print(str.repr(u8.parse(" 255 "))?)?
    print(str.repr(u8.parse("256"))?)?
    print(str.repr(i64.parse("hello"))?)?
    Ok(())
```
```output
Result[u8, ParseError].Ok(255)
Result[u8, ParseError].Err(ParseError.OutOfRange)
Result[i64, ParseError].Err(ParseError.Invalid)
```

Functions returning the same error type can propagate parsing failures with `?`.
[`try_from`](../02-values/numeric-operations.md) reports a value that does not
fit with `RangeError.OutOfRange`. Match it explicitly to combine parsing,
calculation, and narrowing in a function returning `ParseError`:

```plenty
def shade(percent: &str) -> Result[u8, ParseError]:
    scaled = i64.parse(percent)? * 255 // 100
    match u8.try_from(scaled):
        case Ok(value):
            Ok(value)
        case Err(error):
            match error:
                case RangeError.OutOfRange:
                    Err(ParseError.OutOfRange)

def main() -> Result[(), AllocError | IoError]:
    print(str.repr(shade("50"))?)?
    print(str.repr(shade("150"))?)?
    print(str.repr(shade("half"))?)?
    Ok(())
```
```output
Result[u8, ParseError].Ok(127)
Result[u8, ParseError].Err(ParseError.OutOfRange)
Result[u8, ParseError].Err(ParseError.Invalid)
```
