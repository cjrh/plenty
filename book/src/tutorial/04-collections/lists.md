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
    rows[0][-1] += 7
    print(rows)?
    Ok(())
```

```output
[[1, 9], [3]]
```

Nested assignment does not copy or allocate the containing lists.
Numeric elements support augmented assignment such as `+=` too. Each target
index runs once before the right side, which may read the list but cannot change
or resize it during the update. A failed right-side result leaves the element
unchanged.
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

`sum` adds a list of numbers. It starts from zero, leaves the list usable, and
checks integer overflow at every addition:

```plenty
def main() -> Result[(), Failure]:
    prices = [250, 120, 30]?
    weights = [0.5, 0.25]?
    empty: list[u8] = list()?
    print(sum(prices))?
    print(sum(weights))?
    print(sum(empty))?
    print(len(prices))?
    Ok(())
```

```output
400
0.75
0
3
```
