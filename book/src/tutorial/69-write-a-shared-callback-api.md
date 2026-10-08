# Write a shared callback API

A callable constraint lets an API work with named functions and captured closures.
Borrow the callback when the caller should keep it. The callback's concrete
signature can also tell the compiler the return type.

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
inline storage; this shared API needs no heap allocation or dynamic interface.
Use `&mut F` when the callback needs to update captured state.
