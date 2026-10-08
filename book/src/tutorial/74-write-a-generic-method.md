# Write a generic method

A method may declare its own parameters in addition to those of its class.
The arguments determine their concrete types, just as for a generic function.

```plenty
class Cell[T]:
    value: T

    def map[U, F: Callable[[T], U]](self, transform: &F) -> Result[Cell[U], AllocError]:
        Cell(transform(self.value))

def double(value: u8) -> u16:
    u16(value) * 2

def main() -> Result[(), Failure]:
    original = Cell(7u8)?
    transform = double
    changed = original.map(&transform)?
    print(changed.value)?
    print(original.value)?
    Ok(())
```
```output
14
7
```

`map` borrows the original cell. Here its u8 field is copyable. The result is a
new `Cell[u16]`, and its allocation can fail. Method parameters cannot shadow
class parameters. Constructors and destructors use only the class parameters.

When a type appears only in the result, specify it on the method call:

```plenty
class Number:
    value: i64

    def convert[T: IntType](self) -> T:
        T(self.value)

def main() -> Result[(), Failure]:
    number = Number(7)?
    small: u8 = number.convert[u8]()
    print(small)?
    Ok(())
```
```output
7
```
