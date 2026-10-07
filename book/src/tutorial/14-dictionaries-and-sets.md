# 14. Dictionaries and sets

Dictionaries map a single key type to a single value type. Keys and set elements
may be integers, booleans, or strings. Dictionary values can include collections.

```plenty
def main() -> Result[(), Failure]:
    mut scores: dict[str, i64] = {"Ada": 10, "Grace": 20}?
    scores["Ada"] = 12
    scores.insert("Lin", 30)?
    print(scores["Ada"])?
    print("Grace" in scores)?
    print(scores.keys()?)?
    print(scores.values()?)?

    mut names: set[str] = set()?
    names.add("Ada")?
    names.add("Ada")?
    print(len(names))?
    print("Ada" in names)?
    Ok(())
```

```output
12
True
["Ada", "Grace", "Lin"]
[12, 20, 30]
1
True
```

A repeated dictionary key replaces its value and keeps its insertion position.
`keys()` and `values()` return new lists in insertion order. When dictionary
values contain mutable collections, use `copy(scores).values()` to request
independent payloads explicitly. Sets remove
duplicates and have no promised iteration order. `{}` is an empty dictionary;
use an annotation with `set()` or write `set[str]()` for an empty set.

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
The later lessons explain `Option` and its `?` propagation in more detail.

`get` observes the dictionary and key, leaving both available. The lookup itself
does not allocate, so it needs no allocation-error result. String
and immutable enum results retain their existing storage and remain valid even
if the dictionary is subsequently updated or dropped.

`get` supports values such as numbers, booleans, strings, and immutable enums.
It rejects mutable collections, classes, and enums containing them. Use `pop`
to remove those values and take ownership, or use `&dictionary[key]` to borrow
an existing value. Optional borrowed lookup is deferred. No mutable value is
silently copied by `get`.
