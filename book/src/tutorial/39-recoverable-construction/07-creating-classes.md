# Creating classes

A class instance stores its fields inline, so constructing one needs no
allocation. `Class.new(...)` is the same as `Class(...)` and returns the
instance:

```plenty
class Point:
    x: i64
    y: i64

def main() -> Result[(), IoError]:
    print(Point.new(3, 4))?
    Ok(())
```
```output
Point(x=3, y=4)
```

Construction can fail only when the initializer can. An initializer returning
`Result[(), AllocError]` makes both `Class(...)` and `Class.new(...)` return
`Result[Class, AllocError]`:

```plenty
class Buffer:
    values: list[i64]

    def __init__(self: &mut Buffer, size: i64) -> Result[(), AllocError]:
        self.values = list[i64].with_capacity(size)?
        Ok(())

def main() -> Result[(), Failure]:
    print(Buffer.new(8)?)?
    Ok(())
```
```output
Buffer(values=[])
```

If initialization fails, the fields already initialized are dropped, but the
incomplete instance's `__del__` is skipped. Successful initialization must fill
every field and return `Ok(())`.
