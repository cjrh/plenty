# Consume a callback

A reusable library function can accept any callback that it owns and calls once.
Use the `OnceCallable` constraint. It accepts named functions, reusable closures,
and one-shot closures. The signature tells the compiler the result type.

```plenty-file callbacks.plenty
pub def run[T, F: OnceCallable[[], T]](callback: F) -> T:
    mut owned = callback
    owned()
```

```plenty
import callbacks

def defer[T](value: T) -> OnceClosure[[], T]:
    def once [value]() -> T:
        value

def main() -> Result[(), Failure]:
    take = defer([3, 5]?)
    print(callbacks.run(take))?
    Ok(())
```
```output
[3, 5]
```

`OnceClosure` describes the concrete value returned by `defer`. `OnceCallable`
describes what the generic `run` function can accept. Passing `take` transfers
ownership; calling it again afterward is an error. The mutable local inside
`run` also permits reusable callbacks that update private captured state.

For a callback that also borrows local state, invoke it in that local scope:

```plenty
def main() -> Result[(), Failure]:
    mut count = 0
    values = [8]?
    take = def once [values, &mut count]() -> list[i64]:
        count = count + 1
        values
    result = take()
    print(count)?
    print(result)?
    Ok(())
```
```output
1
[8]
```

The borrow ends after the call. Borrowing environments cannot yet move across
function boundaries, so this second callback cannot be passed to `callbacks.run`.
