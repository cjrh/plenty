# Accept functions and closures through one API

A library helper may need to accept both a named function and a closure with
captured data. Constrain a type parameter by its call signature, and borrow the
callback when the caller should keep it. The compiler infers the concrete
environment and argument types without boxing or allocating an environment.

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

Here `T` is inferred as `u8` from both arguments, and `F` is the closure's concrete
type. Conflicting evidence is an error. This consumer also accepts a named
function with the same signature; it does not need a separate closure-only API.
Use `&mut F` for a callback that needs to change its captured state.
Such consumers can also borrow closures with shared or exclusive captures; those
captures remain protected throughout the call.

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

Both calls infer all three type arguments. Captures remain in the closure's
inline storage; sharing the API does not add a dynamic interface.
