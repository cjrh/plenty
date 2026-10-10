# Transform a cell with a generic method

Give a method its own type parameters to let a callback transform `Cell[T]`
into `Cell[U]`:

```plenty
class Cell[T]:
    value: T

    def map[U, F: Callable[[T], U]](self, transform: &F) -> Cell[U]:
        Cell(transform(self.value))

def double(value: u8) -> u16:
    u16(value) * 2

def main() -> Result[(), Failure]:
    original = Cell(7u8)
    transform = double
    changed = original.map(&transform)
    print(changed.value)?
    print(original.value)?
    Ok(())
```
```output
14
7
```

`map` borrows the original cell, so passing its field to the callback requires
a copyable `T`. Here, `double` determines the result type `U = u16`.
