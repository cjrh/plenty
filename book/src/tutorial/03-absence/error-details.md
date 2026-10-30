# Choose which error details to keep

Keep a concrete error type when the caller needs the cause. For example,
`IoError` preserves an input/output cause, while `str` can carry a message.

Pass an error back unchanged when the caller can use it directly. When several
operations have different errors, the [next lesson](anonymous-sums.md) shows
how to keep them together without losing their details.

Sometimes an API needs to add meaning to a lower-level cause. Here,
`ConfigError.InvalidCount` identifies what was being parsed and retains the
original `ParseError`. A Result's `map_error` method changes just the error
type, forwarding success unchanged:

```plenty
enum ConfigError:
    InvalidCount(ParseError)

def read_count(text: &str) -> Result[i64, ConfigError]:
    i64.parse(text).map_error(ConfigError.InvalidCount)

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

`ConfigError.InvalidCount` written without arguments is a function from
`ParseError` to `ConfigError`. `map_error` calls it once with the original
error, only when parsing fails. On success, the parsed count passes through
unchanged. The conversion policy is explicit, and mapping adds no allocation.
`read_count` returns that result directly because it has no more work to do.
Use `?` when further work depends on the successful value. See the [mapping reference](../../design/stdlib/error.md#explicit-error-mapping)
for callback evaluation and ownership details.

When the new error needs more than the cause, such as a second payload or
captured context, pass an [anonymous function](../10-functions/anonymous-functions.md)
that builds it instead.

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
