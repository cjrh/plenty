# Give a method its own type parameters

A method may declare type parameters of its own, in addition to those on its
class. Arguments and the expected result determine them just as they do for
generic functions. Here the annotation on `small` chooses `T`:

```plenty
class Number:
    value: i64

    def convert[T: IntType](self) -> T:
        T(self.value)

def main() -> Result[(), IoError]:
    number = Number(7)
    small: u8 = number.convert()
    print(small)?
    Ok(())
```
```output
7
```

Without an annotation, choose it explicitly: `small = number.convert[u8]()`.
Method parameters cannot shadow class parameters. Constructors and destructors
use only the class parameters. See the [functions-as-values path](../10-functions/index.md)
for a generic method that accepts a callback.
