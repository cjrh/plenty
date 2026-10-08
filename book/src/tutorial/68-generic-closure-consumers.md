# Generic closure consumers

Generic functions can infer types inside a closure signature, including the
result. The environment stays concrete, so this requires no boxing or allocation.

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
