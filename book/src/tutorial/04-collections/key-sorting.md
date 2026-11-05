# Sort by a key or create a sorted copy

`sort_by_key` reads each element through a shared reference. Its key can be a
number, string, or tuple of those, so class and collection elements can be
ordered too. An optional positional `True` requests descending order.

```plenty
class Point:
    x: i64
    y: i64

def horizontal(point: &Point) -> i64:
    point.x

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut points = [Point(3, 1), Point(1, 2), Point(2, 4)]?
    points.sort_by_key(horizontal)
    for point in &points:
        print(point.x)?
    points.sort_by_key(horizontal, True)
    print(points[0].x)?
    points.sort_by_key(def(point: &Point) -> Result[i64, OverflowError[i64]]: Ok(-point.y))?
    print(points[0].x)?
    Ok(())
```

```output
1
2
3
3
2
```

The operation allocates no key buffer. It calls the key repeatedly, so use a
consistent key without side effects. Equal keys may change relative order;
include a tie breaker in a tuple key when the order matters. The key can also
be an anonymous function written in place, as the last call shows, or a
reusable shared-call closure.

`sorted` creates a copy in natural order, leaving its source available. Copying
is fallible and therefore needs `?`:

```plenty
def main() -> Result[(), AllocError | IoError]:
    scores = [72, 95, 58]?
    print(sorted(&scores)?)?
    print(scores)?
    print(sorted(&scores, True)?)?
    Ok(())
```

```output
[58, 72, 95]
[72, 95, 58]
[95, 72, 58]
```

For a copy ordered by a key, explicitly copy into a `mut` binding, then call
`sort_by_key`. Keys may return `Result`; handle the sorting call with `?` and include its error
in the enclosing signature. On failure, elements may already be reordered.
One-shot or mutable-call closures are not supported.
