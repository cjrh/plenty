# Read a list element that might be missing

`items.get(index)` returns `Some(value)` for an existing element or `Nothing`
for an out-of-range index. It takes one `i64` index, including negative indices:

```plenty
def main() -> Result[(), Failure]:
    names = ["Ada", "Bea"]?
    print(names.get(-1))?
    match names.get(2):
        case Some(name):
            print(name)?
        case Nothing:
            print("no name at that position")?
    print(names)?
    Ok(())
```

```output
Option[str].Some("Bea")
no name at that position
["Ada", "Bea"]
```

This is a constant-time read that leaves the list unchanged and allocates nothing.
Numbers and booleans are copied; strings and immutable enum values share their
existing storage. A returned string remains valid after the original list is
updated or dropped. There is no default argument; choose a fallback with `match`.

Like dictionary `get`, this supports scalar and immutable elements. For lists
containing mutable collections or classes, use `pop` to transfer ownership, or
explicitly copy through ordinary indexing. Individual element borrowing is not
implemented yet. Ordinary `items[index]` still traps for an out-of-range index.
