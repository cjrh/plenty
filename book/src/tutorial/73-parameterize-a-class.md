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

def main() -> Result[(), Failure]:
    mut count = Cell[u8](7)?
    count.replace(9)
    print(count.get())?
    text = Cell[str]("hello")?
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
