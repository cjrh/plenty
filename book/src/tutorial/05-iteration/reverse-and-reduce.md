# Traverse backward and test boolean inputs

Use `reversed` directly in a loop or comprehension to visit a list, text,
or range backward. Borrow a list to keep it afterward. Text yields Unicode
scalars, so a multibyte character stays intact.

```plenty
def main() -> Result[(), IoError]:
    values = [10, 20, 30].unwrap()
    for index, value in enumerate(reversed(&values)):
        print((index, *value))?
    for letter in reversed("aé🙂"):
        print(letter)?
    print([x for x in reversed(range(5)) if x % 2 == 0].unwrap())?
    print(values)?
    Ok(())
```

```output
(0, 30)
(1, 20)
(2, 10)
🙂
é
a
[4, 2, 0]
[10, 20, 30]
```

Use `any` to find whether at least one boolean is `True`, or `all` to check
whether every boolean is `True`. Both stop as soon as they know the answer.
An empty input gives `False` for `any` and `True` for `all`.

```plenty
def positive(values: list[i64]) -> Generator[bool]:
    for value in values:
        yield value > 0

def main() -> Result[(), IoError]:
    values = [2, -1, 3].unwrap()
    print(any(positive(copy(values).unwrap())))?
    print(all(positive(values)))?
    flags = [True, True].unwrap()
    print(all(reversed(&flags)))?
    print(any(list[bool]().unwrap()))?
    print(all(list[bool]().unwrap()))?
    Ok(())
```

```output
True
False
True
False
True
```

The named generator computes its comparisons only when advanced. A list
comprehension instead builds every result before `any` or `all` begins.
These functions require booleans; numbers and text have no implicit
truth-value conversion. Borrow existing lists to preserve them; owned
lists and generator frames are consumed, with their remaining state
cleaned up even when reduction stops early.

`reversed` adds no allocation and does not change a list's order. Use
`reversed(&mut values)` to edit its elements in reverse order. It currently
accepts only lists, text, and ranges as loop/comprehension sources, and as
the input to `any` or `all`; it cannot be stored or passed to `next`.
