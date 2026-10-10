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
