# Capture data and private state

When an operation needs more than its call arguments, let its function value
own that extra data. Use an explicit list to capture surrounding bindings:

```plenty
def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    offset = 4
    values = [10, 20]?
    adjust = def [offset, values](index: i64) -> Result[i64, OverflowError[i64]]:
        Ok(values[index] + offset)
    print(adjust(0)?)?
    print(adjust(1)?)?
    Ok(())
```
```output
14
24
```

`values` moves into the closure; the scalar `offset` copies. Each call borrows
the captures, so the list remains available for subsequent calls and drops with
the closure. Its environment is stored inline and has a different type from the
code-only `Callable`. A reusable closure cannot move captures out; borrow or
explicitly `copy` them when an independent owner is needed.

Use `mut` in the capture list for private state. A stateful closure also needs
a mutable binding to call:

```plenty
def main() -> Result[(), IoError | OverflowError[i64]]:
    count = 0
    mut next = def [mut count]() -> Result[i64, OverflowError[i64]]:
        count = count + 1
        Ok(count)
    print(next()?)?
    print(next()?)?
    print(count)?
    Ok(())
```
```output
1
2
0
```

The captured scalar changes independently of the original `count`.
