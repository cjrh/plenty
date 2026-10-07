# Dictionary key/value loops

`items()` borrows a dictionary in insertion order. It works directly in loops
and comprehensions, without allocating item tuples or a snapshot. Owned values
are shared references; scalar and string values remain values. Use an exclusive
borrow to update values. Keys stay immutable.

```plenty
def main() -> Result[(), Failure]:
    mut counts = {"apple": 2, "pear": 3}?
    for fruit, count in (&mut counts).items():
        *count = *count + 1
    print([(fruit, count)? for fruit, count in counts.items()]?)?
    print([a + b for a, b in [(1, 2)?, (3, 4)?]?]?)?
    Ok(())
```
```output
[("apple", 3), ("pear", 4)]
[3, 7]
```

The first comprehension explicitly builds a snapshot of tuples. For dictionaries
containing owned values, use `copy(value)?` when a snapshot
needs independent owned values. The dictionary cannot grow or shrink while an
item loop is using it. An `items()` view cannot yet be stored in a variable.
