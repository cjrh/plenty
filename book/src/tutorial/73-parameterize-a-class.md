# Parameterize a class

A generic class shares one declaration across concrete field types. Its methods
can use the class's type parameters.

```plenty
class Cell[T]:
    value: T

    def get(self) -> T:
        self.value

    def replace(self: &mut Cell[T], value: T) -> ():
        self.value = value

def read[T](cell: &Cell[T]) -> T:
    cell.get()

def main() -> Result[(), Failure]:
    mut count = Cell(7u8)?
    count.replace(9)
    print(read(&count))?
    text = Cell("hello")?
    print(text.get())?
    Ok(())
```
```output
9
hello
```

This `get` method works for integers and immutable strings, which can be read
from a shared field. An owned list needs an explicit copy or a reference; using
`Cell[list[i64]].get` is rejected because it would move out of a borrowed field.
The call to `read` infers T from the argument; no `read[u8]` is required.
Constructors infer their parameters too. `Cell(7u8)` and `Cell[u8](7)` have the
same type. With `Cell(7)`, the unsuffixed integer defaults to i64.
