# Build and update a list

A list contains values of one type. Use `list[T]` in signatures and annotations.
An empty list needs an annotation or a typed constructor such as `list[i64]()`.

```plenty
def main() -> Result[(), Failure]:
    mut original: list[i64] = [10, 20]?
    mut changed = copy(original)?
    changed.append(30)?
    changed[0] = 99
    print(original)?
    print(changed)?
    print(changed[-1])?
    print(len(changed))?
    Ok(())
```

```output
[10, 20]
[99, 20, 30]
30
3
```

`copy(original)?` makes an independent list. Updates happen in place and
require a `mut` owner. `append` grows the list; indexed assignment replaces
an existing element. Negative indices count from the end, and an invalid index
is a runtime error.

List construction, copying, and growth return allocation
[results](../03-absence/index.md), handled here with `?`.

Collections can nest. Update an inner list directly through its indices:

```plenty
def main() -> Result[(), Failure]:
    mut rows = [[1, 2]?, [3]?]?
    rows[0][-1] = 9
    print(rows)?
    Ok(())
```

```output
[[1, 9], [3]]
```

Nested assignment does not copy or allocate the containing lists.
Indices can be expressions, including elements of another list:

```plenty
def main() -> Result[(), Failure]:
    mut values = [30, 10, 20]?
    order = [1, 2, 0]?
    print(values[order[0]])?
    values[order[2]] = 31
    print(values)?
    Ok(())
```

```output
10
[31, 10, 20]
```
