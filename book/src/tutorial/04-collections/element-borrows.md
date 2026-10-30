# Borrowing collection elements

Borrow an element when you want to inspect or change it without moving its owner:

```plenty
def main() -> Result[(), AllocError | IoError]:
    mut counts = [1, 5]?
    value = &mut counts[0]
    *value = 2
    print(counts)?
    Ok(())
```
```output
[2, 5]
```

The loan ends after its last use. While an element reference is live, the
collection cannot grow, remove entries, or move. An out-of-range index traps.
Dictionaries lend a value through
[`get_mut`](dictionaries.md), which returns an `Option` instead of trapping on
a missing key.

Two list elements written as different number literals are different places, so
both can be borrowed at once:

```plenty
def main() -> Result[(), AllocError | IoError]:
    mut scores = [10, 20, 30]?
    first = &mut scores[0]
    last = &mut scores[2]
    *first = *first + *last
    *last = 0
    print(scores)?
    Ok(())
```
```output
[40, 20, 0]
```

The compiler proves only this literal case. Any other index, such as `i` or
`-1`, may name the same element, so its loan covers the whole list:

```plenty-error
def main() -> Result[(), AllocError | IoError]:
    mut scores = [10, 20, 30]?
    i = 2
    first = &mut scores[0]
    last = &mut scores[i]
    *first = *first + *last
    Ok(())
```
```error
while `scores[0]` is exclusively borrowed
```

Dictionary entries are never separated this way. To exchange two list elements
at computed indices, [`swap`](reordering.md) them instead.

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

def main() -> Result[(), AllocError | IoError]:
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

## Borrow part of a list

`&values[a:b]` borrows a run of elements in place. This *slice view* copies
nothing, so unlike a [copying slice](slices.md) it needs no `?`. A function
takes a view as `&[T]`, or as `&mut [T]` to change the elements:

```plenty
def total(values: &[i64]) -> i64:
    mut sum = 0
    for value in values:
        sum += *value
    sum

def reset(values: &mut [i64]) -> ():
    for value in values:
        *value = 0

def main() -> Result[(), AllocError | IoError]:
    mut scores = [70, 85, 90, 65, 88]?
    print(total(&scores[1:]))?
    reset(&mut scores[:2])
    print(scores)?
    Ok(())
```

```output
328
[0, 0, 90, 65, 88]
```

The positions clamp as a copying slice's do. A view supports `len`, indexing,
narrower views, loops, and `==` with another view. A list is not converted to a
view automatically: pass a whole list as `&scores[:]`. A view borrows its whole
list, so the list cannot grow while the view is live, and two views of one list
conflict when either is exclusive. Byte buffers have views too. A view cannot
be returned or stored yet; the
[slice reference](../../design/language/04-expressions/slices.md#slice-views)
lists its operations and limits.
