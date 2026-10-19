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
variants; [`case _ as name:`](bind-the-rest.md) also binds the remaining value. Duplicate or missing cases are errors:

```plenty-error
enum Switch:
    On
    Off

def main() -> Result[(), AllocError | IoError]:
    match Switch.On:
        case Switch.On:
            print("on")?
    Ok(())
```

```error
non-exhaustive match; missing Switch.Off
```

A final match produces the function's result; cases can also return early or
break/continue a loop. A match can also be
[the value of a binding](match-values.md). Separately declared enums remain distinct types, even
with identical variants.

A payload position can hold another pattern instead of a name, so one case
reaches into an enum stored inside an enum. `|` joins patterns that share an
arm; every alternative must bind the same names:

```plenty
enum Shape:
    Dot
    Circle(i64)
    Rect(i64, i64)

enum Slot:
    Empty
    Full(Shape)

def width(slot: Slot) -> i64:
    match slot:
        case Slot.Full(Shape.Circle(size) | Shape.Rect(size, _)):
            size
        case Slot.Full(Shape.Dot) | Slot.Empty:
            0

def main() -> Result[(), Failure]:
    print(width(Slot.Full(Shape.Rect(3, 4))))?
    print(width(Slot.Full(Shape.Circle(5))))?
    print(width(Slot.Empty))?
    Ok(())
```

```output
3
5
0
```

Coverage is checked through the nested patterns: leaving out
`Slot.Full(Shape.Dot)` is an error that names it.

Payloads can contain strings, collections, and other enums. Matching an enum
with mutable payloads consumes it and transfers those payloads; use
`match copy(value)?` to preserve the owner.

An enum can keep behavior beside its variants. A method's bare `self` is a
shared borrow, so matching it borrows the payload. The same method can satisfy
a structural protocol:

```plenty
protocol Numbered:
    def number(self) -> i64:
        pass

enum Reading:
    Missing
    Value(i64)

    def number(self) -> i64:
        match self:
            case Reading.Missing:
                0
            case Reading.Value(value):
                *value

def number[T: Numbered](value: &T) -> i64:
    value.number()

def main() -> Result[(), IoError]:
    reading = Reading.Value(42)
    print(number(&reading))?
    print(reading.number())?
    Ok(())
```

```output
42
42
```

Use `pub def` when callers in other modules need the method, and
`self: &mut Reading` when it changes a payload. Variant and method names must
be distinct. The [formatting lesson](../08-resources/formatting.md) uses an enum
method to produce a message while preserving the error data.
