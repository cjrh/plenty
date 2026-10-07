# Generic function values

A higher-order function can use a type parameter inside its callable signature.
The compiler infers the type from the function value and the other arguments:

```plenty
def increment(value: u8) -> u8:
    value + 1

def apply[T](operation: Callable[[T], T], value: T) -> T:
    operation(value)

def main() -> Result[(), Failure]:
    print(apply(increment, 41u8))?
    Ok(())
```
```output
42
```

Here `T` is `u8`. Passing `41i64` would conflict with `increment`'s signature.

To choose a generic function's own type arguments, use brackets without calling it:

```plenty
def identity[T](value: T) -> T:
    value

def main() -> Result[(), Failure]:
    keep = identity[u8]
    print(keep(42))?
    Ok(())
```
```output
42
```

`identity[u8]` is a function value; `identity[u8](42)` calls that specialization.
