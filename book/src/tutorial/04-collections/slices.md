# Take part of a list or text

A slice copies part of a list or string into a new value. Write the start and
stop positions inside the brackets, separated by `:`. The start is included and
the stop is excluded:

```plenty
def main() -> Result[(), AllocError | IoError]:
    scores = [70, 85, 90, 65, 88]?
    print(scores[1:3]?)?
    print(scores[:2]?)?
    print(scores[3:]?)?
    print(scores[-2:]?)?
    print(scores[::-1]?)?
    print(scores)?
    Ok(())
```

```output
[85, 90]
[70, 85]
[65, 88]
[65, 88]
[88, 65, 90, 85, 70]
[70, 85, 90, 65, 88]
```

An omitted start or stop runs to that end of the list. A negative position
counts from the end, so `scores[-2:]` takes the last two elements. A third
number is the step: `[::2]` takes every second element and `[::-1]` walks
backwards. The source list is unchanged.

A slice allocates its new list, so it returns a `Result` like a
[list literal](lists.md) and needs `?`:

```plenty-error
def main() -> Result[(), AllocError | IoError]:
    scores = [70, 85, 90]?
    top: list[i64] = scores[:2]
    Ok(())
```

```error
slice returns Result[list[i64], AllocError]; use `?` to propagate allocation failure
```

Positions outside the list do not stop the program. They clamp to its ends, as
in Python, so `scores[3:100]` and `scores[100:]` are fine; the second is empty.

## Slices are copies

A slice is an [explicit copy](moves-and-copies.md). Each selected element is
copied, including nested lists, so changing the source afterwards does not
change the slice:

```plenty
def main() -> Result[(), AllocError | IoError]:
    mut rows = [[1, 2]?, [3]?]?
    first = rows[:1]?
    rows[0].append(99)?
    print(first)?
    print(rows)?
    Ok(())
```

```output
[[1, 2]]
[[1, 2, 99], [3]]
```

Copying costs time and memory in proportion to the selected elements. To read
elements without copying them, borrow them as a slice view, `&scores[1:]`, which
the [element borrowing lesson](element-borrows.md#borrow-part-of-a-list)
introduces. A loop over their positions also avoids the copy:

```plenty
def main() -> Result[(), AllocError | IoError]:
    scores = [70, 85, 90, 65, 88]?
    mut total = 0
    for i in range(1, len(scores)):
        total += scores[i]
    print(total)?
    Ok(())
```

```output
328
```

## Text

String slices count Unicode characters, the same positions as `len` and
indexing, so a slice never splits a character:

```plenty
def main() -> Result[(), AllocError | IoError]:
    word = "café!"
    print(word[:4]?)?
    print(word[-1:]?)?
    print(word[::-1]?)?
    Ok(())
```

```output
café
!
!éfac
```

The step must not be zero. A literal `0` is a compile error, and a computed step
is converted with `NonZero.try_from(step)?`, which reports zero as an error. The
[slice reference](../../design/language/04-expressions/slices.md) covers steps,
tuples, bytes, columnar lists, and views.
