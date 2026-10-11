# Sort and reorder a list

`sort()` puts a list in ascending order. `sort(True)` gives descending order.
Both change the list in place, need a `mut` owner, and allocate nothing, so
neither takes `?`.

```plenty
def main() -> Result[(), Failure]:
    mut scores = [72, 95, 58, 81]?
    scores.sort()
    print(scores)?
    scores.sort(True)
    print(scores)?
    mut names = ["pear", "fig", "apple"]?
    names.sort()
    print(names)?
    Ok(())
```

```output
[58, 72, 81, 95]
[95, 81, 72, 58]
["apple", "fig", "pear"]
```

`sort` orders integers, floats, strings, and tuples of those. Tuples compare
their first components, then the next where the first are equal. To sort
records by a key, put the key first:

```plenty
def main() -> Result[(), Failure]:
    mut ranked = [(72, "Ada"), (95, "Bea"), (72, "Abe")]?
    ranked.sort()
    print(ranked)?
    print(ranked[-1][1])?
    Ok(())
```

```output
[(72, "Abe"), (72, "Ada"), (95, "Bea")]
Bea
```

Other element types need a [key function](key-sorting.md); natural sorting rejects them:

```plenty-error
def main() -> Result[(), Failure]:
    mut flags = [True, False]?
    flags.sort()
    Ok(())
```
```error
sort does not order bool elements; it orders integers, floats, str, and tuples of those
```

## Exchange two elements

`swap(i, j)` exchanges two elements of any type. Negative indices count from
the end, and a missing index is a runtime error.

```plenty
def main() -> Result[(), Failure]:
    mut rows = [[0, 1]?, [2, 3]?, [4, 5]?]?
    rows.swap(0, -1)
    print(rows)?
    Ok(())
```

```output
[[4, 5], [2, 3], [0, 1]]
```

The rows are exchanged where they are. Nothing is copied.

## Assign several targets at once

An assignment can name several targets, separated by commas. The whole right
side is evaluated first, and then each value is assigned to its target from
left to right. That makes an exchange a single statement:

```plenty
def main() -> Result[(), Failure]:
    mut values = [10, 20, 30]?
    values[0], values[2] = values[2], values[0]
    print(values)?
    mut low = 9
    mut high = 4
    low, high = high, low
    print((low, high))?
    Ok(())
```

```output
[30, 20, 10]
(4, 9)
```

The right side is a [tuple](tuples.md) written without its parentheses. Reading
`values[2]` copies an integer. A nested list is owned by its row list and cannot
be moved out by reading it, so this form does not exchange owned elements:

```plenty-error
def main() -> Result[(), Failure]:
    mut rows = [[0, 1]?, [2, 3]?]?
    rows[0], rows[1] = rows[1], rows[0]
    Ok(())
```
```error
cannot move out of indexed storage; use copy(collection[index])
```

Use `rows.swap(0, 1)` instead.
