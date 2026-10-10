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
    for fruit, count in counts.items():
        print(fruit)?
        print(count)?
    for a, b in [(1, 2), (3, 4)]?:
        print(a + b)?
    Ok(())
```
```output
apple
3
pear
4
3
7
```

Both loops unpack a two-position tuple into two names. The dictionary cannot
grow or shrink while an item loop is using it. An `items()` view cannot yet be
stored in a variable. The comprehension lesson uses the same unpacking syntax
to collect a snapshot.
