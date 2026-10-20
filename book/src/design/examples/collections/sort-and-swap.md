# Sort a list and exchange elements

`sort()` orders a list in place without allocating. Tuples sort by their first
component, so sorting `(start, stop)` spans puts overlapping spans next to each
other. Merging them is then one pass:

```plenty
def merge(spans: &mut list[(i64, i64)]) -> Result[list[(i64, i64)], AllocError]:
    spans.sort()
    mut merged: list[(i64, i64)] = []?
    for i in range(len(spans)):
        start, stop = spans[i]
        if len(merged) > 0 and start <= merged[-1][1]:
            if stop > merged[-1][1]:
                merged[-1] = (merged[-1][0], stop)
        else:
            merged.append((start, stop))?
    Ok(merged)

def main() -> Result[(), AllocError | IoError]:
    mut spans = [(8, 10), (1, 3), (15, 18), (2, 6)]?
    print(merge(&mut spans)?)?
    print(spans)?
    Ok(())
```

```output
[(1, 6), (8, 10), (15, 18)]
[(1, 3), (2, 6), (8, 10), (15, 18)]
```

`swap(i, j)` exchanges two elements of any type in place. Here it moves the row
with the largest leading value to the top, as elimination with pivoting does,
without copying a row:

```plenty
def pivot(rows: &mut list[list[i64]]) -> ():
    mut best = 0
    for i in range(1, len(rows)):
        if rows[i][0] > rows[best][0]:
            best = i
    rows.swap(0, best)

def main() -> Result[(), AllocError | IoError]:
    mut rows = [[1, 2]?, [7, 8]?, [4, 5]?]?
    pivot(&mut rows)
    print(rows)?
    Ok(())
```

```output
[[7, 8], [1, 2], [4, 5]]
```

Copyable elements can also be exchanged by
[assigning several targets](../../language/04-expressions/assignment.md#several-targets):
`items[i], items[j] = items[j], items[i]`.
