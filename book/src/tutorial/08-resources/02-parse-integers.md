# Reading integers from text

Use a sized type's `parse` method for external text. It borrows the string and
returns an allocation-free `Result`. Decimal ASCII digits and an optional sign
are accepted, with surrounding Unicode whitespace ignored. Numeric prefixes,
underscores, and non-ASCII digits are not accepted.

```plenty
def main() -> Result[(), Failure]:
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
fit with the same `ParseError.OutOfRange`, so one function can parse a wide
integer, calculate, and narrow the result:

```plenty
def shade(percent: &str) -> Result[u8, ParseError]:
    scaled = i64.parse(percent)? * 255 // 100
    Ok(u8.try_from(scaled)?)

def main() -> Result[(), Failure]:
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
