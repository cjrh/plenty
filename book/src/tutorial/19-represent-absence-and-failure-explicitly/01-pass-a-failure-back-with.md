# Pass a failure back with `?`

Put `?` after a `Result` expression to extract its `Ok` payload or return its
`Err` immediately. This keeps a sequence of fallible operations easy to read:

```plenty
def validate(name: str) -> Result[(), str]:
    if len(name) == 0:
        return Err("name is empty")
    Ok(())

def checked_name(name: str) -> Result[str, str]:
    validate(name)?
    Ok(name)

def main() -> Result[(), IoError]:
    print(checked_name("Plenty"))?
    print(checked_name(""))?
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
