# Take dictionary snapshots with recoverable allocation

`keys()` and `values()` build new lists in dictionary insertion order,
returning `Result[list[T], AllocError]`. They take no arguments. Use `?` to
propagate allocation failure:

```plenty
def names(scores: &dict[str, i64]) -> Result[list[str], AllocError]:
    result = scores.keys()?
    Ok(result)

def main() -> Result[(), Failure]:
    mut scores = {"Ada": 10, "Bea": 20}?
    saved = scores.values()
    scores["Ada"] = 30
    print(names(&scores))?
    print(saved)?
    print(scores)?
    Ok(())
```

```output
Result[list[str], AllocError].Ok(["Ada", "Bea"])
Result[list[i64], AllocError].Ok([10, 20])
{"Ada": 30, "Bea": 20}
```

For numbers, strings, and other immutable values, the dictionary remains usable
after either outcome. Strings and immutable enum storage are shared; the new
list does not copy their contents. The snapshot remains valid after the source
changes or leaves scope. An empty dictionary produces `Ok([])` if its new list
header can be allocated.

For owned values such as lists or classes, `values()` requires an owned
temporary, just like `values()`. Request duplication explicitly to keep the
original:

```plenty
def rows(data: &dict[str, list[i64]]) -> Result[list[list[i64]], AllocError]:
    copy(data)?.values()

def main() -> Result[(), Failure]:
    data = {"first": [1, 2]?, "second": [3]?}?
    print(rows(&data))?
    print(data)?
    Ok(())
```

```output
Result[list[list[i64]], AllocError].Ok([[1, 2], [3]])
{"first": [1, 2], "second": [3]}
```

Calling `make_dictionary().values()` instead transfers owned values from
that temporary into the list. If allocation fails, the temporary and its values
are cleaned up. `keys()` never takes the dictionary's values, so it works on
borrowed dictionaries regardless of their value type.

Both operations reserve all list storage before retaining or transferring any
elements. They report `OutOfMemory` or `CapacityOverflow` through `AllocError`.
There are no separate aborting snapshot methods.
