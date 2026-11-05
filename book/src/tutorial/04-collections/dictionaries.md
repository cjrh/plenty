# Associate keys with values

A `dict[K, V]` maps keys of one type to values of another. Keys may be integers,
booleans, strings, bytes, or tuples of supported keys; values can include collections.
Named enums can also be keys when their payloads are supported key types; see
[heterogeneous dictionaries](../heterogeneous-collections/dictionaries.md).
The `keys()` and `values()` loops below borrow their elements: `*name` reads
the value behind a reference. The [borrowing lesson](borrowing.md)
explains references in detail.

```plenty
def main() -> Result[(), AllocError | IoError]:
    mut scores: dict[str, i64] = {"Ada": 10, "Grace": 20}?
    scores.insert("Ada", 12)?
    scores.insert("Lin", 30)?
    print(scores["Ada"])?
    print("Grace" in scores)?
    for name in scores.keys():
        print(*name)?
    for score in scores.values():
        print(*score)?

    Ok(())
```

```output
12
True
Ada
Grace
Lin
12
20
30
```

`insert` adds a key or replaces the value of an existing one, which keeps its
insertion position. Adding a key can allocate, so `insert` returns an
allocation result. Dictionaries have no indexed assignment: `scores["Ada"] = 12`
is a compile error. Reading `scores["Ada"]` traps when the key is missing.

To change a value in place, `get_mut` returns `Option[&mut V]`: a mutable
reference when the key is present, or `Nothing`:

```plenty
def main() -> Result[(), AllocError | IoError]:
    mut groups = {"first": [10, 20]?}?
    match groups.get_mut("first"):
        case Some(group):
            group[0] = 11
            group.append(30)?
        case Nothing:
            pass
    print(groups)?
    Ok(())
```

```output
{"first": [11, 20, 30]}
```

`get_mut` allocates nothing. The reference borrows the whole dictionary
exclusively, so the dictionary cannot be read or changed while it is live.

To count or group, a missing key needs a starting value. `get_or_insert(key,
default)` inserts the default when the key is absent, then returns a mutable
reference to the stored value. It finds the key with one lookup. Inserting can
allocate, so it returns a `Result`; `?` propagates the `AllocError`:

```plenty
def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut counts: dict[str, i64] = {}?
    for word in ["red", "blue", "red"]?:
        *counts.get_or_insert(word, 0)? += 1
    print(counts)?
    Ok(())
```

```output
{"red": 2, "blue": 1}
```

The default is built before the lookup, even when the key is present. A
default like `0` costs nothing; one that allocates, such as `[]?`, allocates on
every call, so use `get_mut` when the key is usually present.

`keys()` and `values()` borrow elements in insertion order without allocating.
The references protect the dictionary from mutation while they are used.
Keep a key view in a local when checking several properties:

```plenty
def main() -> Result[(), AllocError | IoError]:
    scores = {"Ada": 12, "Grace": 20}?
    names = scores.keys()
    expected = {"Ada", "Grace", "Lin"}?
    print(len(names), "Ada" in names, names.issubset(&expected))?
    print(names.union(&expected)? == expected)?
    Ok(())
```
```output
2 True True
True
```

Read-only checks return plain values. Set algebra and `names.to_set()` allocate
an owned set and return an allocation result. The view stays local: use
`scores.keys_snapshot()?` or `scores.values_snapshot()?` when an owned ordered
list must outlive the borrow. For mutable payloads,
`copy(groups)?.values_snapshot()?` requests independent copies explicitly.
`into_keys()` and `into_values()` consume a dictionary into an ordered list.
See the [full view contract](../../design/facilities/01-builtins/dictionaries.md#snapshots-and-views)
for restrictions and cost details. `{}` is an empty dictionary; select its types
with an annotation or a constructor such as `dict[str, i64]()?`.

## Reuse a borrowed view

Bind a shared view when several observations use the same dictionary. The
view keeps the dictionary borrowed until its last use; it creates no snapshot.

```plenty
def main() -> Result[(), AllocError | IoError]:
    scores = {"Ada": 10, "Grace": 20}?
    values = scores.values()
    entries = scores.items()
    print(len(values), len(entries))?
    for score in values:
        print(*score)?
    for name, score in entries:
        print(*name, *score)?
    print(iter(values).count())?
    Ok(())
```
```output
2 2
10
20
Ada 10
Grace 20
2
```

Use `iter(view)` for transformations and terminals. Item-view callbacks take
a key and a value parameter, just like direct `items()` pipelines.
