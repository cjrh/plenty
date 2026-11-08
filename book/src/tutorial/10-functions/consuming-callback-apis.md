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

def main() -> Result[(), AllocError | IoError]:
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

## Queue one-shot jobs

`OnceClosure` keeps each producer distinct, so jobs from different factories
cannot share a list. Box each one with `BoxedOnceCallable` to queue them
together. A call consumes the box, so take each job out of the list first;
consuming iteration does that:

```plenty
def greet(name: str) -> Result[BoxedOnceCallable[[], Result[(), AllocError | IoError]], AllocError]:
    job = def once [name]() -> Result[(), AllocError | IoError]:
        print(f"hello {name}"?)
    BoxedOnceCallable(job)

def collect(values: list[i64]) -> Result[BoxedOnceCallable[[], Result[(), AllocError | IoError]], AllocError]:
    job = def once [values]() -> Result[(), AllocError | IoError]:
        print(values)
    BoxedOnceCallable(job)

def main() -> Result[(), AllocError | IoError]:
    jobs = [greet("ada")?, collect([1, 2]?)?, greet("lin")?]?
    for job in jobs:
        job()?
    Ok(())
```

```output
hello ada
[1, 2]
hello lin
```

Each job runs once; its captures drop when it finishes. A job left in the list
drops with the list without running.
