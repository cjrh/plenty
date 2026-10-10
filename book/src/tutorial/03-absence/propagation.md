# Pass a failure back with `?`

Put `?` after a `Result` expression to extract its `Ok` payload or return its
`Err` immediately. This keeps a sequence of fallible operations easy to read.
The example uses `Failure` to deliberately discard any error details while
reporting that work failed; the next lesson explains that choice.


```plenty
def validate(name: str) -> Result[(), str]:
    if len(name) == 0:
        return Err("name is empty")
    Ok(())

def checked_name(name: str) -> Result[str, str]:
    validate(name)?
    Ok(name)

def main() -> Result[(), Failure]:
    print(str.repr(checked_name("Plenty"))?)?
    print(str.repr(checked_name(""))?)?
    Ok(())
```

```output
Result[str, str].Ok("Plenty")
Result[str, str].Err("name is empty")
```

Here `validate(name)?` has a unit success value, so it can stand alone. On
failure, the final `Ok(name)` never runs. Live local values and previously
evaluated expression temporaries are cleaned up automatically, just as for
an explicit `return`.

`str.repr` explicitly formats each wrapper here so both outcomes are visible.
Ordinary `print` requires you to handle a Result first; use
`print(checked_name("Plenty")?)?` when you want just the successful name.
An expression statement cannot silently discard a Result either:

```plenty-error
def main() -> Result[(), Failure]:
    print("hello")
    Ok(())
```
```error
cannot implicitly discard a Result
```

Add `?` to propagate the error, or match the result to recover. Use
`drop(result)` only when ignoring either outcome is intentional. This check
does not diagnose unused bindings or discard of `Option` or other values.

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
but `Result` error types must match exactly unless the function explicitly
chooses `Failure`, described below. Use `match` when you need to convert errors
while preserving details. `?` is not supported inside generators.

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

`Option` and `Result` wrappers do not allocate on the heap, including when
nested. Their payloads keep their usual behavior: `Some([1, 2]?)` allocates the
list, but adds no wrapper allocation. Returning or propagating an existing sum
does not allocate a wrapper either. Printing and operations on the payload can
still allocate, and report failures through their own results.
