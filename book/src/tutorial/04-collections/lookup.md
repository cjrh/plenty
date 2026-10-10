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

A stored zero, `False`, empty string, or `Nothing` still counts as a present
value. For example, looking up a stored `Nothing` returns `Some(Nothing)`.
There is no optional default argument; use a match to choose a fallback.
[Absence and failure](../03-absence/index.md) explains the same `Option`
pattern independently of collections.

`get` observes the dictionary and key, leaving both available. The lookup itself
does not allocate, so it needs no allocation-error result. String
and immutable enum results retain their existing storage and remain valid even
if the dictionary is subsequently updated or dropped.

`get` supports values such as numbers, booleans, strings, and immutable enums.
It rejects mutable collections, classes, and enums containing them. Use `pop`
to remove those values and take ownership, or use `&dictionary[key]` to borrow
an existing value. Optional borrowed lookup is deferred. No mutable value is
silently copied by `get`.

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

This is a constant-time read that leaves the list unchanged and allocates nothing.
Numbers and booleans are copied; strings and immutable enum values share their
existing storage. A returned string remains valid after the original list is
updated or dropped. There is no default argument; choose a fallback with `match`.

Like dictionary `get`, this supports scalar and immutable elements. For lists
containing mutable collections or classes, use `pop` to transfer ownership, or
explicitly copy through ordinary indexing. Use `&items[index]` to borrow an existing element. Optional borrowed
lookup is not implemented. Ordinary `items[index]` still traps for an out-of-range index.
