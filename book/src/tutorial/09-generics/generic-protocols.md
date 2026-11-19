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

def main() -> Result[(), IoError]:
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

## Name a type determined by a method

Use an associated type when one of the types in a protocol is determined by
its implementation. `Source` names its result item as `Item`. A generic bound
can refer to that type as `S.Item`.

```plenty
protocol Source:
    type Item

    def read(self) -> Option[Item]:
        pass

protocol Sink:
    type Input

    def put(self: &mut Sink, value: Input) -> ():
        pass

class Counter:
    def read(self) -> Option[i64]:
        Some(1)

class Cell:
    value: i64

    def put(self: &mut Cell, value: i64) -> ():
        self.value = value

def read_once[S: Source](source: &S) -> Option[S.Item]:
    source.read()

def transfer_one[S: Source, D: Sink[Input = S.Item]](
    source: &S, dest: &mut D
) -> ():
    match source.read():
        case Some(item):
            dest.put(item)
        case Nothing:
            pass

def main() -> Result[(), IoError]:
    counter = Counter()
    print(read_once(&counter))?
    mut cell = Cell(0)
    transfer_one(&counter, &mut cell)
    print(cell.value)?
    Ok(())
```
```output
Option[i64].Some(1)
1
```

The signatures give `Counter` the item type `i64` and `Cell` the input type
`i64`. The equality on `D` makes `transfer_one` require those types to agree.
A sink whose `put` takes `str` fails at the call. Conformance is structural,
so the classes only define their methods. See the
[associated-type reference](../../design/language/07-generics/protocols.md#associated-types)
for the permitted bounds and where projections can be used.
