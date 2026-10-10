# Discard error details deliberately

When callers only need success or failure, declare `Result[T, Failure]`.
Each `?` can then propagate a different error type:

```plenty
def work(text: str) -> Result[list[u8], Failure]:
    n = u8.parse(text)?               # ParseError
    values = [n, n + 1u8]?           # AllocError
    print("built values")?          # IoError
    Ok(values)

def main() -> Result[(), Failure]:
    print(str.repr(work("41"))?)?
    print(str.repr(work("invalid"))?)?
    Ok(())
```

```output
built values
Result[list[u8], Failure].Ok([41, 42])
Result[list[u8], Failure].Err(Failure.Unspecified)
```

`Failure.Unspecified` is the type's only value. On an error path, `?` drops the
original error and propagates this marker, with normal scope cleanup. The
conversion and marker do not allocate; custom cleanup can still perform its
own operations. This works in helpers as well as `main`. A `main` that returns
`Err` exits with status one after reporting the error on standard error. For
`Failure` that report can only say that the details were discarded:

```text
error: main returned Failure.Unspecified: Failure keeps no details of the original error
```

This choice discards details, so use a concrete error type when a caller needs
to inspect or report the cause. It only changes `?`: returning an existing
`Result[T, IoError]` directly from a `Result[T, Failure]` function is still a type
error. To report failure yourself, use `Err(Failure.Unspecified)`. End successful
paths explicitly with `Ok(value)` or `Ok(())`.

`Failure` does not catch runtime traps and does not turn `Option.Nothing` into an
error. `.unwrap()` is a separate, explicit choice to terminate immediately on
`Err` or `Nothing`, without normal scope cleanup. For example:

```plenty
def main() -> Result[(), IoError]:
    found = Some(42)
    print(found.unwrap())?
    Ok(())
```

```output
42
```

Use `?` for propagation, `match` for recovery, and `.unwrap()` only when
termination is the intended policy.
