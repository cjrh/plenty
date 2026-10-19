# Pass a failure back with `?`

Put `?` after a `Result` expression to extract its `Ok` payload or return its
`Err` immediately. The example's `main` preserves allocation and printing errors
in `AllocError | IoError`; `checked_name` keeps its concrete `str` error.

```plenty
def validate(name: str) -> Result[(), str]:
    if len(name) == 0:
        return Err("name is empty")
    Ok(())

def checked_name(name: str) -> Result[str, str]:
    validate(name)?
    Ok(name)

def main() -> Result[(), AllocError | IoError]:
    print(str.repr(checked_name("Plenty"))?)?
    print(str.repr(checked_name(""))?)?
    Ok(())
```

```output
Result[str, str].Ok("Plenty")
Result[str, str].Err("name is empty")
```

`validate(name)?` stops `checked_name` on failure, skipping `Ok(name)`. Error
propagation cleans up local values and temporaries, just as `return` does.

`str.repr` formats a result wrapper so either outcome can be printed.
Ordinary `print` requires a handled result:
`print(checked_name("Plenty")?)?` prints only the successful name.
An expression statement cannot silently discard a result:

```plenty-error
def main() -> Result[(), AllocError | IoError]:
    print("hello")
    Ok(())
```
```error
cannot implicitly discard a Result
```

Propagate with `?`, match to recover, or use `drop(result)` to intentionally
ignore either outcome. The discard check does not cover unused bindings or
`Option` values.

`?` works with `Option` too: it extracts `Some` or immediately returns `Nothing`.

```plenty
def positive(n: i64) -> Option[i64]:
    Some(n) if n > 0 else Nothing

def doubled(n: i64) -> Option[i64]:
    Some(positive(n)? * 2)

def main() -> Result[(), IoError]:
    print(doubled(21))?
    print(doubled(-1))?
    Ok(())
```

```output
Option[i64].Some(42)
Option[i64].Nothing
```

The enclosing function must return the same family: `Result` for a `Result`
operand or `Option` for an `Option` operand. Success payload types can differ,
but a `Result` error must either match the declared error type or fit within
its [anonymous sum](../../design/language/03-types/anonymous-sums.md).
For example, `Result[(), AllocError | IoError]` lets the same function propagate
list allocation failures and printing failures with `?`. The sum retains the
original error and adds no allocation. Use `match` when you want to recover or
translate an error rather than pass it back.
`?` is not supported inside generators.

```plenty-error
def read_number() -> Result[i64, str]:
    Err("not a number")

def checked() -> Result[i64, i64]:
    Ok(read_number()?)

def main() -> ():
    print(checked()).unwrap()
```

```error
`?` requires identical Result error types
```

`Option` and `Result` wrappers do not allocate. Their payloads can:
`Some([1, 2]?)` allocates the list, with no additional allocation for `Some`.
