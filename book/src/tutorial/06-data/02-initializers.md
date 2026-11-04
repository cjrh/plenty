# Control construction with __init__

Define `__init__` to calculate or check fields during construction. It replaces
the generated constructor; bare `self` is an exclusive reference:

```plenty
class Span:
    start: i64
    end: i64

    def __init__(self, start: i64, length: i64) -> Result[(), OverflowError[i64]]:
        self.start = start
        self.end = start + length
        Ok(())

    def length(self) -> Result[i64, OverflowError[i64]]:
        Ok(self.end - self.start)

def main() -> Result[(), IoError | OverflowError[i64]]:
    span = Span(10, 5)?
    print(span)?
    print(span.length()?)?
    Ok(())
```

```output
Span(start=10, end=15)
5
```

The initializer calculates the endpoint from a start and length. Overflow returns
its declared `OverflowError[i64]`, so `Span(10, 5)` returns
`Result[Span, OverflowError[i64]]`; `?` propagates failure through `main`.
An initializer returning `()` instead gives an infallible constructor. Construction
itself allocates nothing and adds no allocation error; see
[constructor contracts](../../design/language/03-types/classes.md).

Every field must be initialized on every path out of the constructor. You may
read a field already initialized, but cannot pass the whole unfinished `self` to
another function or method. A loop might never execute, so initializing a field
only inside a loop is insufficient.

```plenty-error
class Pair:
    first: i64
    second: i64

    def __init__(self, first: i64) -> ():
        self.first = first

def main() -> ():
    pass
```

```error
fields not initialized: second
```
