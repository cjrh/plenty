# Look up a value that may be absent

Missing keys in `scores[key]` are runtime errors. Use `scores.get(key)` when
absence is expected. It returns an `Option`: `Some(value)` if found, or `Nothing`
if absent. Match the two possibilities explicitly:

```plenty
def main() -> Result[(), Failure]:
    scores = {"Ada": 12, "Grace": 0}?
    match scores.get("Ada"):
        case Some(score):
            print(score)?
        case Nothing:
            print("unknown player")?
    print(scores.get("Grace"))?
    print(scores.get("Lin"))?
    print(scores["Ada"])?
    Ok(())
```

```output
12
Option[i64].Some(0)
Option[i64].Nothing
12
```

A stored zero, `False`, empty string, or `Nothing` counts as present; looking
up a stored `Nothing` returns `Some(Nothing)`. Use the
[option match](../03-absence/index.md) to choose a fallback; `get` has no
default argument.

`get` leaves the dictionary and key available and does not allocate. It
supports numbers, booleans, strings, and immutable enums. It cannot return
mutable collections or classes: use `pop` to take ownership or
`&dictionary[key]` to borrow an existing value. Optional borrowed lookup is not
implemented.

## Read a list element that might be missing

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

List `get` has the same element restrictions as dictionary `get`. For owned
elements, use `pop` to transfer ownership, `copy(items[index])?` to duplicate
one, or `&items[index]` to borrow it. Ordinary indexing still traps when the
index is out of range.

Both forms of `get` leave their source unchanged. Numbers and booleans copy;
strings and immutable enums share storage. The returned value remains valid
if the source is updated or dropped.
