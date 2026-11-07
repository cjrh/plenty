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
consistent key without side effects. The key can also be an anonymous function
written in place, as the last call shows, or a reusable shared-call closure.

`sort_by_key` may change the relative order of elements with equal keys. When
that order matters, as for records arriving in time order, use
`sort_stable_by_key`. It keeps equal keys in their original order, and costs a
scratch buffer for half the list, so it returns `Result[(), AllocError]`:

```plenty
from std.time import Timestamp

class Event:
    at: Timestamp
    name: str

def when(event: &Event) -> (i64, u32):
    (event.at.seconds, event.at.nanoseconds)

def main() -> Result[(), AllocError | IoError]:
    mut events = [
        Event(Timestamp(20, 0), "deploy"),
        Event(Timestamp(10, 0), "build"),
        Event(Timestamp(20, 0), "notify"),
        Event(Timestamp(10, 0), "test"),
    ]?
    events.sort_stable_by_key(when)?
    for event in &events:
        print(event.name)?
    Ok(())
```

```output
build
test
deploy
notify
```

A `Timestamp` is not a sort key itself; the tuple of its seconds and
nanoseconds orders the same way. If the buffer cannot be allocated, the list is
unchanged. A list already in key order needs no buffer.

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
`sort_by_key` or `sort_stable_by_key`. Keys may return `Result`; handle the sorting call with `?` and include its error
in the enclosing signature. On failure, elements may already be reordered.
One-shot or mutable-call closures are not supported.
