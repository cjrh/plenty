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

`match` handles the alternatives explicitly. In the first branch, `value`
names the payload and belongs to that branch. The second branch handles absence.
Cover both variants; handling only the successful case leaves a possible path
unaccounted for.

`Some` and `Nothing` are built in and need no type prefix. `Nothing`
belongs to one concrete option type rather than acting as a universal null.
A present zero, `False`, or empty string is still `Some(value)`.
