# Keep several error types

A sum keeps one complete value from a finite set of types. Use `|` in the
error type when a function combines operations with different errors:

```plenty
type SmallNumberError = ParseError | RangeError

def parse_small(text: str) -> Result[u8, SmallNumberError]:
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

Use a module-level [type alias](../../design/language/03-types/aliases.md) to
name a recurring group: `SmallNumberError` means exactly `ParseError |
RangeError`. It introduces no wrapper or allocation, so `?` and exhaustive
matching still work on the original members. An alias also composes with
other errors: `SmallNumberError | IoError` is the same sum as `ParseError |
RangeError | IoError`.

Name the errors the function actually propagates. An operation may already
wrap a lower-level error in its own enum; that does not require adding the
lower-level type separately. A new wrapper enum is useful when an API wants
to hide or reorganize its causes, but naming a group needs only an alias.

A [`checked:` block](../02-values/numeric-operations.md#recover-from-overflow)
returns one of these sums. Its errors are classes, values with named fields,
and a class pattern matches one and binds its fields in order:

```plenty
def report(total: u8, count: u8) -> Result[(), IoError]:
    result = checked:
        rounded = total + count // 2
        rounded // count
    match result:
        case Ok(mean):
            print(mean)?
        case Err(OverflowError(wrapped)):
            print("overflow, wrapped to", wrapped)?
        case Err(ZeroDivisionError()):
            print("no values")?
    Ok(())

def main() -> Result[(), IoError]:
    report(200, 4)?
    report(250, 20)?
    report(250, 0)?
    Ok(())
```

```output
50
overflow, wrapped to 4
no values
```

`OverflowError(wrapped)` selects the overflow member and binds its one field.
`ZeroDivisionError()` has no fields, so its parentheses are empty. Write `_`
for a field you do not need.
[Classes](../06-data/01-records-and-methods.md) explains fields, and
[class patterns](../../design/language/04-expressions/matching.md#class-patterns)
gives the complete rules.

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
