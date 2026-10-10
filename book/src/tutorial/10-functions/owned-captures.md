# Capture data and private state

When an operation needs more than its call arguments, let its function value
own that extra data. Use an explicit list to capture surrounding bindings:


```plenty
def main() -> Result[(), Failure]:
    offset = 4
    values = [10, 20]?
    adjust = def [offset, values](index: i64) -> i64:
        values[index] + offset
    print(adjust(0))?
    print(adjust(1))?
    Ok(())
```
```output
14
24
```

`values` moves into the closure; `offset` is a scalar and copies normally.
The closure owns its captures and drops them when it leaves scope. Calling it
borrows that environment, so both calls work without copying the list. Creating
the closure itself does not allocate. Its concrete environment type differs from
the code-only `Callable` type. Captures cannot be moved out of the body; borrow
them or explicitly `copy` when an independent owner is needed.

Use `mut` in the capture list for private state. A stateful closure also needs
a mutable binding to call:

```plenty
def main() -> Result[(), Failure]:
    count = 0
    mut next = def [mut count]() -> i64:
        count = count + 1
        count
    print(next())?
    print(next())?
    print(count)?
    Ok(())
```
```output
1
2
0
```

The captured scalar is independent of the original scalar. An owned collection
instead moves into the environment, just as it does on ordinary assignment.
