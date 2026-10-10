# Control construction with __init__

Define `__init__` to calculate or check fields during construction. It replaces
the generated constructor; bare `self` is an exclusive reference:

```plenty
class Span:
    start: i64
    end: i64

    def __init__(self, start: i64, length: i64) -> ():
        self.start = start
        self.end = self.start + length

    def length(self) -> i64:
        self.end - self.start

def main() -> Result[(), Failure]:
    span = Span(10, 5)
    print(span)?
    print(span.length())?
    Ok(())
```

```output
Span(start=10, end=15)
5
```

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
