# Print, unpack, and use tuple keys

Print several values with spaces between them. Unpack an owned list when its
length is known; the list must contain exactly as many elements as targets.
The statement moves its elements, and `_` discards an element.

```plenty
def main() -> Result[(), AllocError | IoError]:
    first, second = "north east".split(" ")?
    print(first, second, 2)?
    mut visits = dict[(i64, i64), i64].new()?
    cell = (3, 4)
    visits.insert(cell, visits.get(cell, 0) + 1)?
    visits.insert(cell, visits.get(cell, 0) + 1)?
    print(visits.get(cell, 0))?
    visited = {(3, 4), (3, 4), (4, 3)}?
    print(len(visited), cell in visited)?
    Ok(())
```
```output
north east 2
2
2 True
```

Tuple keys contain integers, booleans, strings, or further tuples of those
types. Values compare by their contents; a grid coordinate needs no integer
encoding or string allocation. `insert` adds or replaces a dictionary value
with a recoverable allocation error. Indexed assignment only replaces an
existing key.

Consume dictionary values directly when the dictionary is finished:

```plenty
def main() -> Result[(), AllocError | IoError]:
    groups = {"first": [1, 2]?, "second": [3]?}?
    for group in groups.into_values()?:
        print(group)?
    Ok(())
```
```output
[1, 2]
[3]
```

`into_values()` consumes the dictionary and transfers its owned values into
an ordered list. Use `copy(groups)?` first if the original is still needed.
