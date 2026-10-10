# Iterate over collections and text

`for` visits list elements, dictionary keys, set elements, range integers, or
characters of a string. A character is a one-character `str`.

```plenty
def main() -> Result[(), Failure]:
    mut total = 0
    for n in range(1, 5):
        total = total + n
    print(total)?

    scores = {"Ada": 10, "Grace": 20}?
    for name in &scores:
        print(name)?
        print(scores[name])?

    print(list(range(5, 0, -2))?)?
    for character in "hé":
        print(character)?
    Ok(())
```

```output
10
Ada
10
Grace
20
[5, 3, 1]
h
é
```

`range(stop)` starts at zero; the stop is excluded. A step may be negative but
never zero. Ranges can be reused and do not allocate; `list(range(5))?` allocates
the list. String iteration yields characters without allocating.

Ranges default to `i64`. Use `range[u8](8)` or a typed bound to choose another
element type. Start and stop must fit it; the step remains signed `i64`.

The iterable is evaluated once. Iterating an owned collection transfers it into
the loop; use `for item in &values` to preserve the owner. Borrowed iteration
prevents conflicting mutation; owned list elements become shared references.
Use `&mut values` for mutable element references. To iterate a snapshot while
mutating the original, use `for item in copy(values)?`.
