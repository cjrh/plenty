# Generic closure consumers

Generic functions can infer types inside a closure signature, including the
result. The environment stays concrete, so this requires no boxing or allocation.

```plenty
def apply[T](f: &Closure[[T], T], value: T) -> T:
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

Here `T` is inferred as `u8` from both arguments. Conflicting evidence is an error.
Use `&mut Closure[...]` for a callback that needs to change its captured state.
Such consumers can also borrow closures with shared or exclusive captures; those
captures remain protected throughout the call.
