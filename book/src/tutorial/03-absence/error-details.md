# Choose which error details to keep

Keep a concrete error type when the caller needs the cause. For example,
`IoError` preserves an input/output cause, while `str` can carry a message.

`Result[T, Failure]` deliberately discards the cause: each `?` may accept a
different error type, drop it, and return `Failure.Unspecified`.

```plenty
def validate(name: str) -> Result[(), str]:
    if len(name) == 0:
        return Err("name is empty")
    Ok(())

def announce(name: str) -> Result[(), Failure]:
    validate(name)?
    print(name)?
    Ok(())

def main() -> Result[(), IoError]:
    match announce(""):
        case Ok(_):
            print("announced")?
        case Err(error):
            print(error)?
    Ok(())
```

```output
Failure.Unspecified
```

Use `match` to convert or report errors when you need their details.

This conversion applies to `?`. Returning a `Result[T, IoError]` directly
from a function declared `Result[T, Failure]` is still a type error.
Construct a deliberate failure with `Err(Failure.Unspecified)`.

If `main` returns an error, the program reports it on standard error and exits
with status one. A `Failure` report can only say that the original details
were discarded. `Failure` does not catch runtime traps or turn
`Option.Nothing` into an error.

`.unwrap()` explicitly extracts a successful payload or terminates on
`Err` or `Nothing`, without normal scope cleanup:

```plenty
def main() -> Result[(), IoError]:
    found = Some(42)
    print(found.unwrap())?
    Ok(())
```

```output
42
```

Choose `.unwrap()` only when termination is your intended policy.
