# Consume a capture

A reusable closure borrows its captured state on each call. Sometimes the purpose
of the callback is to hand over an owned value. Mark that callback `once`:

```plenty
def main() -> Result[(), Failure]:
    values = [2, 3, 5]?
    take = def once [values]() -> list[i64]:
        values
    result = take()
    print(result)?
    Ok(())
```
```output
[2, 3, 5]
```

`take()` consumes the callback and transfers its captured list to the result.
A second call is a use-after-move error. If `take` is never called, dropping it
cleans up the list.
