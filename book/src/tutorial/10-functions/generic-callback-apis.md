# Accept functions and closures through one API

Constrain a type parameter by its call signature to accept named functions and
captured closures through one parameter. Borrow the callback when its caller
should keep it:

```plenty
def apply[T, F: Callable[[T], T]](f: &F, value: T) -> T:
    f(value)

def make[T: IntType](offset: T) -> Closure[[T], T]:
    def [offset](value: T) -> T:
        offset + value

def main() -> Result[(), Failure]:
    add = make(2u8)
    print(apply(&add, 3u8))?
    Ok(())
```
```output
5
```

`F` preserves the callback's concrete environment type. Both arguments must
agree on `T`; conflicting type evidence is an error. Use `&mut F` for a callback
that changes its captured state. Borrowed captures stay protected throughout
the call.

## Let the callback determine a separate result type

The first helper requires the argument and result to have the same type `T`.
Give the result its own parameter `U` to allow transformations between types.
The callback's concrete signature determines `U`, including when it does not
appear in another argument. This example uses both forms of callback through
that shared interface:

```plenty
def apply[T, U, F: Callable[[T], U]](callback: &F, value: T) -> U:
    callback(value)

def double(value: i64) -> i64:
    value * 2

def main() -> Result[(), Failure]:
    function = double
    offset = 7
    add = def [offset](value: i64) -> i64:
        offset + value
    print(apply(&function, 10))?
    print(apply(&add, 10))?
    Ok(())
```
```output
20
17
```
