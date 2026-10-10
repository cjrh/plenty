# Consume a callback

Use `OnceCallable` for a function that owns and calls its callback once. It accepts
named functions, reusable closures, and one-shot closures.

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
