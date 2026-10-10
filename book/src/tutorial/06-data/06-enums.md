# Describe alternatives with enums

An enum defines alternatives, each of which may carry typed data. Match its
variants to extract that data:

```plenty
enum Reading:
    Missing
    Value(i64)
    Invalid(str)

def describe(reading: Reading) -> str:
    match reading:
        case Reading.Missing:
            "no reading"
        case Reading.Value(number):
            "positive" if number > 0 else "nonpositive"
        case Reading.Invalid(reason):
            reason

def main() -> Result[(), Failure]:
    print(describe(Reading.Missing))?
    print(describe(Reading.Value(42)))?
    print(describe(Reading.Invalid("sensor offline")))?
    Ok(())
```

```output
no reading
positive
sensor offline
```

Enum payloads are stored inline: `Reading.Value(42)` needs no allocation or `?`.
Variants without data omit parentheses. Payloads may have several positions,
such as `Pair(i64, str)`. Case bindings are immutable and local to that case.
Use `_` to ignore a payload position or a final `case _:` to cover remaining
variants. Duplicate or missing cases are errors:

```plenty-error
enum Switch:
    On
    Off

def main() -> ():
    match Switch.On:
        case Switch.On:
            print("on").unwrap()
```

```error
non-exhaustive match; missing Switch.Off
```

A final match produces the function's result; cases can also return early or
break/continue a loop. Separately declared enums remain distinct types, even
with identical variants.

Payloads can contain strings, collections, and other enums. Matching an enum
with mutable payloads consumes it and transfers those payloads; use
`match copy(value)?` to preserve the owner.
