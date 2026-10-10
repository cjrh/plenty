# Put a callback in an enum

An enum can also own a callback. Match a borrow when you want to leave the owner
in place, or match the value to transfer its environment into an arm.

```plenty
enum Job[F: Callable[[], i64]]:
    Run(F)
    Empty

def package[F: Callable[[], i64]](callback: F) -> Job[F]:
    Job[F].Run(callback)

def invoke[F: Callable[[], i64]](job: &Job[F]) -> i64:
    match &job:
        case Job[F].Run(callback):
            callback()
        case Job[F].Empty:
            0

def main() -> Result[(), Failure]:
    value = 42
    callback = def [value]() -> i64:
        value
    job = package(callback)
    print(invoke(&job))?
    Ok(())
```

```output
42
```
