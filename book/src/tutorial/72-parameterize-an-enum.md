# Parameterize an enum

Use a type parameter when the same alternatives should carry different kinds of
values. Choose the concrete payload type in brackets.

```plenty
enum Choice[T]:
    Empty
    Value(T)

def main() -> Result[(), Failure]:
    selected = Choice[u8].Value(7)?
    match selected:
        case Choice[u8].Empty:
            print("empty")?
        case Choice[u8].Value(value):
            print(value)?
    Ok(())
```
```output
7
```

Like other user-defined enums, constructing a value can fail to allocate. The
`?` propagates that failure. The payload keeps its normal move and copy rules.
