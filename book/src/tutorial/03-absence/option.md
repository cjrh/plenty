# Represent absence

Use `Option[T]` when a function may have no value to return. It has two
variants: `Some(value)` holds a value of type `T`; `Nothing` has no payload.

```plenty
def positive(n: i64) -> Option[i64]:
    Some(n) if n > 0 else Nothing

def show(n: i64) -> Result[(), IoError]:
    match positive(n):
        case Some(value):
            print(value)?
        case Nothing:
            print("not found")?
    Ok(())

def main() -> Result[(), IoError]:
    show(-1)?
    show(42)?
    Ok(())
```

```output
not found
42
```

In `case Some(value)`, the pattern binds the payload to `value` within that
branch. A `match` must cover both `Some` and `Nothing`.

`Some` and `Nothing` are built in and need no type prefix. `Nothing`
belongs to one concrete option type rather than acting as a universal null.
A present zero, `False`, or empty string is still `Some(value)`.
