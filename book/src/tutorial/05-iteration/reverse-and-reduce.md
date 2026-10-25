# Traverse backward

Use `reversed` directly in a loop or comprehension to visit a list, text,
or range backward. Borrow a list to keep it afterward. Text yields Unicode
scalars, so a multibyte character stays intact.

```plenty
def main() -> Result[(), AllocError | IoError]:
    values = [10, 20, 30]?
    for index, value in enumerate(reversed(&values)):
        print((index, *value))?
    for letter in reversed("aé🙂"):
        print(letter)?
    print([x for x in reversed(range(5)) if x % 2 == 0]?)?
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

`reversed` adds no allocation and does not change a list's order. Use
`reversed(&mut values)` to edit its elements in reverse order. It currently
accepts only lists, text, and ranges as loop/comprehension sources and
[pipeline](pipelines.md) sources; it cannot be stored or passed to `next`.
