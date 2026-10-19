# Keep several error types

A sum keeps one complete value from a finite set of types. Use `|` in the
error type when a function combines operations with different errors:

```plenty
def parse_small(text: str) -> Result[u8, ParseError | RangeError]:
    number = i64.parse(text)?
    u8.try_from(number)

def main() -> Result[(), IoError]:
    match parse_small("300"):
        case Ok(number):
            print(number)?
        case Err(cause):
            match cause:
                case parse: ParseError:
                    print("not an integer")?
                case range: RangeError:
                    print("outside u8 range")?
    Ok(())
```

```output
outside u8 range
```

`?` forwards the original parse error. The direct return widens the conversion
result to the declared set. Neither step creates a heap wrapper or loses the
payload. `case name: Type:` selects one whole member and binds it; every
member needs a case unless you use `_`. An enum member can then be matched
again by its constructors.

Sums also work outside errors:

```plenty
def describe(value: i64 | str) -> Result[(), IoError]:
    match value:
        case number: i64:
            print(number)?
        case text: str:
            print(text)?
    Ok(())

def main() -> Result[(), AllocError | IoError]:
    describe(42)?
    describe("ready")?
    Ok(())
```

```output
42
ready
```

The parameter supplies the destination type, so either member can be passed
directly. `str | i64` means the same type as `i64 | str`; duplicate members
collapse. To inspect without moving an owned payload, match `&value`; use
`&mut value` to change the active payload through a mutable borrow.
