# Dictionary key/value loops

`items()` borrows a dictionary in insertion order. It works directly in loops
and comprehensions, without allocating item tuples or a snapshot. Keys and
values are shared references. Borrow the dictionary exclusively,
`(&mut counts).items()`, to update every value in place; keys stay shared.

```plenty
def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut counts = {"apple": 2, "pear": 3}?
    for fruit, count in (&mut counts).items():
        *count += 1
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

`(&mut counts).values()` lends only the values. Neither loop looks a key up
or copies a value, so they suit values that cannot be copied. The dictionary
cannot grow, shrink, or be read through another path during a mutable loop.
A shared `items()` or `values()` view can be bound to a name and reused; a
mutable one is used directly as the loop source. See
[updating every value](../../design/facilities/01-builtins/dictionaries.md#updating-every-value)
for the full contract.
