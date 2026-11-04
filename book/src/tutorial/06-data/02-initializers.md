# Control construction with __init__

Define `__init__` to calculate or check fields during construction. It replaces
the generated constructor; bare `self` is an exclusive reference:

```plenty
class Span:
    start: i64
    end: i64

    def __init__(self, start: i64, end: i64) -> ():
        self.start = start
        self.end = end

    def length(self) -> Result[i64, OverflowError[i64]]:
        Ok(self.end - self.start)

def main() -> Result[(), IoError | OverflowError[i64]]:
    end = 10 + 5
    span = Span(10, end)
    print(span)?
    print(span.length()?)?
    Ok(())
```

```output
Span(start=10, end=15)
5
```

The application calculates the endpoint before construction, so overflow can
propagate through `main`. This initializer only assigns endpoints. Initializers
currently return only `()` or `Result[(), AllocError]`, not arithmetic errors;
see [constructor contracts](../../design/language/03-types/classes.md).

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
