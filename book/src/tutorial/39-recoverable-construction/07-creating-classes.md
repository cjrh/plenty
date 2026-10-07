# Creating classes

Use `Class.new(...)` to handle failure to allocate instance storage. It takes
the same arguments as the ordinary constructor and works with `?`:

```plenty
class Point:
    x: i64
    y: i64

def point() -> Result[Point, AllocError]:
    Ok(Point.new(3, 4)?)

def main() -> Result[(), IoError]:
    print(point())?
    Ok(())
```
```output
Result[Point, AllocError].Ok(Point(x=3, y=4))
```

Arguments move into the call even when allocation fails. Failure drops those
arguments; it does not run the new instance's initializer or destructor.
Allocations inside argument expressions and an ordinary `__init__` retain their
own failure behavior.

An initializer can also propagate allocation failures itself:

```plenty
class Buffer:
    values: list[i64]

    def __init__(self: &mut Buffer, size: i64) -> Result[(), AllocError]:
        self.values = list[i64].with_capacity(size)?
        Ok(())

def main() -> Result[(), IoError]:
    print(Buffer.new(8))?
    Ok(())
```
```output
Result[Buffer, AllocError].Ok(Buffer(values=[]))
```

Both `Buffer(...)` and `Buffer.new(...)` return the checked construction result.
If initialization fails, the fields already
initialized are dropped, but the incomplete instance's `__del__` is skipped.
Successful initialization must fill every field and return `Ok(())`.
