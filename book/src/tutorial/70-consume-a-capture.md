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

`take()` consumes the callback. A second call would be a use-after-move error.
The list moves from `values` into the callback, then into `result`; none of those
moves duplicates or reallocates its contents. If `take` leaves scope without being
called, it drops the captured list automatically.
