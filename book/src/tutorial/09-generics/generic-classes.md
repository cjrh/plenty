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

def main() -> Result[(), IoError]:
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
returning it would move it out of a borrowed field. A `Cell` can hold a list,
but its `get` cannot be called:

```plenty-error
class Cell[T]:
    value: T

    def get(self) -> T:
        self.value

def main() -> Result[(), AllocError | IoError]:
    numbers = Cell([1, 2]?)
    first = numbers.get()
    Ok(())
```
```error
lesson.plenty:9:13: `Cell[list[i64]].get` copies `T` implicitly, but `list[i64]` is affine
lesson.plenty:5:9: note: in instantiation `Cell[list[i64]].get`: cannot move out of a field
```

A generic method is checked once for each set of type arguments it is used
with, not where it is written. Without the `get` call, the program is accepted.
The error is reported at the line of your code that first required
`Cell[list[i64]].get`, here the method call, and names the type argument that
the method cannot accept. The note below it gives the line in the definition
and the reason.
