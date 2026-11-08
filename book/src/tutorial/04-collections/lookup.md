# Look up a value that may be absent

Missing keys in `scores[key]` are runtime errors. Use `scores.get(key)` when
absence is expected. It returns an `Option`: `Some(value)` if found, or `Nothing`
if absent. Match the two possibilities explicitly:

```plenty
def main() -> Result[(), AllocError | IoError]:
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
supports numbers, booleans, strings, and enums of such values. It cannot return
mutable collections or classes; [borrow them instead](#borrow-a-value-that-may-be-absent)
or use `pop` to take ownership.

## Read a list element that might be missing

`items.get(index)` returns `Some(value)` for an existing element or `Nothing`
for an out-of-range index. It takes one `i64` index, including negative indices:

```plenty
def main() -> Result[(), AllocError | IoError]:
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
elements, borrow them as shown below, use `pop` to transfer ownership, or
`copy(items[index])?` to duplicate one. Ordinary indexing still traps when the
index is out of range. The report gives the source position, the index, and the
length: `error: main.plenty:3:11: index out of bounds: index 5, length 3`.

Both forms of `get` leave their source unchanged. Numbers, booleans, and
enums copy; strings share immutable storage. The returned value remains valid
if the source is updated or dropped.

## Borrow a value that may be absent

`get_ref` lends a stored value instead of copying it, so it works for lists,
classes, and every other value. It returns `Option[&V]`: a reference when the
key or index exists, or `Nothing`. `get_mut` returns `Option[&mut V]` to change
the value in place. Dictionaries and lists provide both:

```plenty
def main() -> Result[(), AllocError | IoError]:
    teams = {"red": ["Ada", "Bea"]?}?
    match teams.get_ref("red"):
        case Some(members):
            print(len(members), members)?
        case Nothing:
            print("no such team")?
    print(teams.get_ref("blue").is_some())?
    mut rows = [[1, 2]?, [3]?]?
    match rows.get_mut(-1):
        case Some(row):
            row.append(4)?
        case Nothing:
            pass
    print(rows)?
    Ok(())
```

```output
2 ["Ada", "Bea"]
False
[[1, 2], [3, 4]]
```

Neither copies nor allocates. List indices count from the end when negative,
as in indexing. While the reference is live it borrows the whole collection,
as [`get_mut` on a dictionary](dictionaries.md) does: with `get_ref` the
collection can still be read but not changed, and with `get_mut` it cannot be
used at all. In the `Nothing` case there is no reference, so that branch may
change the collection, for example to insert the missing entry.
