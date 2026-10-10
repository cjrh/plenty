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

`range(stop)` starts at zero. `range(start, stop, step)` permits a negative
step, but never zero. The stop value is excluded. A range is a small inline value:
creating, copying, and iterating it require no heap allocation. `range(5)` returns
a range directly; `list(range(5))?` handles the allocation needed for the list.
You can iterate the same range repeatedly. String iteration yields one
character at a time without allocating.

Ranges default to `i64`; `range[u8](8)` explicitly produces u8 values. Start
and stop must fit the chosen type. Step remains signed i64, so unsigned ranges
can descend too. A typed bound can also select the element type.

The iterable is evaluated once. Iterating an owned collection transfers it into
the loop; use `for item in &values` to preserve the owner. Borrowed iteration
prevents conflicting mutation; owned list elements become shared references.
Use `&mut values` for mutable element references. To iterate a snapshot while
mutating the original, request it explicitly with `for item in copy(values)`.
Generators are consumed by iteration. Loop variables and new body bindings do not
escape the loop; changes to enclosing `mut` bindings persist. A loop has unit
result. `return` can exit a containing function from a loop. Use the `break` and `continue` statements from the earlier loop lessons.
The next lesson introduces tuple unpacking and dictionary `items()`.
