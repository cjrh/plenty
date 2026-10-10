# Give a method its own type parameters

A method may declare type parameters of its own, in addition to those on its
class. Arguments determine them just as they do for generic functions. A
parameter that appears only in the result must be chosen explicitly:


```plenty
class Number:
    value: i64

    def convert[T: IntType](self) -> T:
        T(self.value)

def main() -> Result[(), Failure]:
    number = Number(7)
    small: u8 = number.convert[u8]()
    print(small)?
    Ok(())
```
```output
7
```

Method parameters cannot shadow class parameters. Constructors and destructors
use only the class parameters. The [functions-as-values path](../10-functions/index.md)
later uses a generic method to transform a cell with a callback.
