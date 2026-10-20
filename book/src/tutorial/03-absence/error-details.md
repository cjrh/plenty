# Choose which error details to keep

Keep a concrete error type when the caller needs the cause. For example,
`IoError` preserves an input/output cause, while `str` can carry a message.

Pass an error back unchanged when the caller can use it directly. When several
operations have different errors, the [next lesson](anonymous-sums.md) shows
how to keep them together without losing their details.

Sometimes an API needs to add meaning to a lower-level cause. Here,
`ConfigError.InvalidCount` identifies what was being parsed and retains the
original `ParseError`. `std.error.map_error` changes just the error type,
forwarding success unchanged:

```plenty
from std.error import map_error

enum ConfigError:
    InvalidCount(ParseError)

def count_error(cause: ParseError) -> ConfigError:
    ConfigError.InvalidCount(cause)

def read_count(text: &str) -> Result[i64, ConfigError]:
    map_error(i64.parse(text), count_error)

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
explicit, and the helper adds no allocation. `read_count` returns that result
directly because it has no more work to do. Use `?` when further work depends on
the successful value. See the [mapping reference](../../design/stdlib/error.md#explicit-error-mapping)
for callback evaluation and ownership details.

The example's `main` deliberately handles a failed parse so you can see that the
cause survived. In an application that cannot continue, propagate the error
instead. Returning an error from `main` reports it on standard error and exits
with status one; printing an error and returning `Ok(())` reports success to the
program's caller. The [command-line application](../08-resources/18-command-line-application.md)
puts this policy into practice.

Avoid replacing useful causes with builtin `Failure`: propagation into
`Result[T, Failure]` discards the original error and keeps only
`Failure.Unspecified`. It is a last resort, not a way to combine errors.
See the reference for its exact
[erasure rules](../../design/language/08-errors/01-sums-and-propagation.md).

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
