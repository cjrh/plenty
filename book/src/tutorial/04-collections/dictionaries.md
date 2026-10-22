# Associate keys with values

A `dict[K, V]` maps keys of one type to values of another. Keys may be integers,
booleans, or strings; values can include collections.
The `keys()` and `values()` loops below borrow their elements: `*name` reads
the value behind a reference. The [borrowing lesson](borrowing.md)
explains references in detail.

```plenty
def main() -> Result[(), AllocError | IoError]:
    mut scores: dict[str, i64] = {"Ada": 10, "Grace": 20}?
    scores["Ada"] = 12
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

A repeated dictionary key replaces its value and keeps its insertion position.
Indexed assignment updates an existing key; `insert` can add a new one and
returns an allocation result. Nested values update directly too:

```plenty
def main() -> Result[(), AllocError | IoError]:
    mut groups = {"first": [10, 20]?}?
    groups["first"][0] = 11
    print(groups)?
    Ok(())
```

```output
{"first": [11, 20]}
```

Indexed assignment allocates nothing. A missing key or an invalid list index
is a runtime error.

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
