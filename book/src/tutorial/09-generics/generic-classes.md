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
    mut count = Cell(7u8)
    count.replace(9)
    print(read(&count))?
    text = Cell("hello")
    print(text.get())?
    Ok(())
```
```output
9
hello
```

Constructors infer their parameters: `Cell(7u8)` and `Cell[u8](7)` have the
same type. With `Cell(7)`, the unsuffixed integer defaults to `i64`.

This `get` method works for integers and immutable strings, which can be read
from a shared field. An owned list needs an explicit copy or a reference:
returning it would move it out of a borrowed field. So `Cell` cannot hold a
list:

```plenty-error
class Cell[T]:
    value: T

    def get(self) -> T:
        self.value

def main() -> Result[(), Failure]:
    numbers = Cell([1, 2]?)
    Ok(())
```
```error
lesson.plenty:8:15: cannot instantiate `Cell[list[i64]]`: its definition is rejected
lesson.plenty:5:9: note: in instantiation `Cell[list[i64]]`: cannot move out of a field
```

A generic definition is checked once for each set of type arguments it is used
with, not where it is written. The error is reported at the line of your code
that first required `Cell[list[i64]]`, here the constructor call, even though
`get` is never called. The note below it gives the line in the definition and
the reason.
