# Borrowing collection elements

Borrow an element when you want to inspect or change it without moving its owner:

```plenty
def main() -> Result[(), Failure]:
    mut counts = {"visits": 1}?
    value = &mut counts["visits"]
    *value = 2
    print(counts)?
    Ok(())
```
```output
{"visits": 2}
```

The loan ends after its last use. While an element reference is live, the
collection cannot grow, remove entries, or move. Different indices are treated
as potentially overlapping. Missing keys and out-of-range indices still trap.

Borrowed loops always give references, including when the elements are numbers.
Printing and comparison inspect those references directly. Use `*` when a
function takes a value or a new collection needs the element itself:

```plenty
def count_eq[T](values: &list[T], target: &T) -> i64:
    mut count = 0
    for value in values:
        if value == target:
            count = count + 1
    count

def main() -> Result[(), Failure]:
    values = [1, 2, 1]?
    target = 1
    print(count_eq(&values, &target))?
    for value in &values:
        print(*value)?
    Ok(())
```

```output
2
1
2
1
```

The same body works for a `list[str]`. Shared dictionary `items()` loops likewise
give `&K` keys and `&V` values; mutable views give `&mut V` values. A borrowed
collection stays protected against changes throughout its loop.
