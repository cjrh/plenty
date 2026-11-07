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
        case Err(ParseError.Invalid):
            print("not an integer")?
        case Err(ParseError.OutOfRange):
            print("outside i64 range")?
        case Err(RangeError.OutOfRange):
            print("outside u8 range")?
    Ok(())
```

```output
outside u8 range
```

`?` forwards the original parse error. The direct return widens the conversion
result to the declared set. Neither step creates a heap wrapper or loses the
payload. Each variant pattern selects its enum member and then its variant,
so the errors can be handled directly inside `Err`. Every variant of every
member needs coverage unless you use `_`.

Use `case name: Type:` when you need one whole member and its value. The
[matching reference](../../design/language/04-expressions/matching.md) describes
how whole-member patterns and variant patterns combine. To handle some members and
[forward the rest](#recover-from-one-error-and-forward-the-rest), name the
remaining members in the last case.

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

[Heterogeneous collections](../heterogeneous-collections/index.md) uses sums
and enum wrappers to store different kinds of values in lists and dictionaries.

## Recover from one error and forward the rest

To handle some members and pass the others on, end the match with
`case name: B | C:`, naming the members the earlier cases leave:

```plenty
enum SettingError:
    Missing

def setting(text: &str) -> Result[u8, SettingError | ParseError | RangeError]:
    if text == "":
        return Err(SettingError.Missing)
    number = i64.parse(text)?
    u8.try_from(number)

def setting_or_default(text: &str) -> Result[u8, ParseError | RangeError]:
    match setting(text):
        case Ok(value):
            Ok(value)
        case Err(error):
            match error:
                case SettingError.Missing:
                    Ok(8)
                case rest: ParseError | RangeError:
                    Err(rest)

def show(text: &str) -> Result[(), IoError]:
    match setting_or_default(text):
        case Ok(value):
            print(value)?
        case Err(error):
            print("invalid setting:", error)?
    Ok(())

def main() -> Result[(), IoError]:
    show("")?
    show("12")?
    show("300")?
    Ok(())
```

```output
8
12
invalid setting: RangeError.OutOfRange
```

A missing setting becomes the default `8`; parse and range errors keep their
original values. `rest` has type `ParseError | RangeError`, so `Err(rest)`
matches the smaller declared error. The compiler checks the written members
against what the earlier cases leave and names the correct set when they
differ. A member counts as handled only when every one of its variants is, so
after `case ParseError.Invalid:` alone the residual still includes
`ParseError`. When one member remains, write the ordinary `case rest: RangeError:`.
The [anonymous-sum reference](../../design/language/03-types/anonymous-sums.md#residual-members)
gives the full rules, including why a borrowed match cannot narrow its sum.
