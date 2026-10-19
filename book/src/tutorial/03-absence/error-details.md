# Choose which error details to keep

Keep a concrete error type when the caller needs the cause. For example,
`IoError` preserves an input/output cause, while `str` can carry a message.

Prefer concrete errors or [error sums](anonymous-sums.md) in application code.
The legacy `Result[T, Failure]` deliberately discards the cause: each `?` may accept a
different error type, drop it, and return `Failure.Unspecified`.
This example demonstrates the information that erasure loses; it is not the
default pattern for combining errors.

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

Use `match` to inspect or report errors when you need their details. To change
just the error type, `std.error.map_error` forwards success for you:

```plenty
from std.error import map_error

enum ConfigError:
    InvalidCount(ParseError)

def count_error(cause: ParseError) -> ConfigError:
    ConfigError.InvalidCount(cause)

def read_count(text: &str) -> Result[i64, ConfigError]:
    count = map_error(i64.parse(text), count_error)?
    Ok(count)

def main() -> Result[(), IoError]:
    match read_count("many"):
        case Ok(count):
            print(count)?
        case Err(ConfigError.InvalidCount(cause)):
            print(cause == ParseError.Invalid)?
    Ok(())
```

```output
True
```

`count_error` receives the original error once, only when parsing fails. On
success, the parsed count passes through unchanged. The conversion policy is
explicit; `?` still expects `ConfigError` here. The helper adds no allocation.
The result and callback expressions are both evaluated before the helper runs;
an unused callback and any values it owns are dropped on success.

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
