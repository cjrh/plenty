# Associate keys with values

A `dict[K, V]` maps keys of one type to values of another. Keys may be integers,
booleans, or strings; values can include collections.

```plenty
def main() -> Result[(), Failure]:
    mut scores: dict[str, i64] = {"Ada": 10, "Grace": 20}?
    scores["Ada"] = 12
    scores.insert("Lin", 30)?
    print(scores["Ada"])?
    print("Grace" in scores)?
    print(scores.keys()?)?
    print(scores.values()?)?

    Ok(())
```

```output
12
True
["Ada", "Grace", "Lin"]
[12, 20, 30]
```

A repeated dictionary key replaces its value and keeps its insertion position.
Indexed assignment updates an existing key; `insert` can add a new one and
returns an allocation result. Nested values update directly too:

```plenty
def main() -> Result[(), Failure]:
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

`keys()` and `values()` return new lists in insertion order. When dictionary
values contain mutable collections, use `copy(groups)?.values()?` to request
independent payloads explicitly. `{}` is an empty dictionary; select its types
with an annotation or a constructor such as `dict[str, i64]()?`.
