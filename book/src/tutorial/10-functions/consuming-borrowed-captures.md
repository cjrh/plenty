# Consume owned data while borrowing local state

For a callback that also borrows local state, invoke it in that local scope:

```plenty
def main() -> Result[(), Failure]:
    mut count = 0
    values = [8]?
    take = def once [values, &mut count]() -> list[i64]:
        count = count + 1
        values
    result = take()
    print(count)?
    print(result)?
    Ok(())
```
```output
1
[8]
```

The borrow ends after the call. This callback cannot be passed to `callbacks.run`
because a borrowing environment cannot move across function boundaries.
