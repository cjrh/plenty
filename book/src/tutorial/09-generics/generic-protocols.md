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
    cell = Cell(7u8)
    print(read(&cell))?
    Ok(())
```
```output
7
```

The argument determines `R = Cell[u8]`; that class's `read` signature determines
`T = u8`. Its return type and receiver borrowing must match the protocol.
Explicit `read[u8, Cell[u8]](&cell)` also works. Conflicting type evidence is
an error, rather than a reason to widen integers.
