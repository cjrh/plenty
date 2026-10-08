# Parameterize a protocol

A protocol can describe the same operation for several result types. Classes
satisfy it by providing matching methods.

```plenty
protocol Readable[T]:
    def read(self) -> T:
        pass

class Cell[T]:
    value: T
    def read(self) -> T:
        self.value

def read[T, R: Readable[T]](source: &R) -> T:
    source.read()

def main() -> Result[(), Failure]:
    cell = Cell(7u8)?
    print(read[u8, Cell[u8]](&cell))?
    Ok(())
```
```output
7
```

The compiler checks that `Cell[u8].read` returns u8 and borrows its receiver as
the protocol requires. The call is statically resolved and does not allocate.
