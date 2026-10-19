# Parameterize an enum

Use a type parameter when the same alternatives should carry different kinds of
values. Choose the concrete payload type in brackets. Here, `T: IntType` accepts
any supported sized integer. Omit the constraint to allow other stored types.

```plenty
enum Choice[T: IntType]:
    Empty
    Value(T)

def main() -> Result[(), IoError]:
    selected = Choice[u8].Value(7)
    match selected:
        case Choice.Empty:
            print("empty")?
        case Choice.Value(value):
            print(value)?
    Ok(())
```
```output
7
```

A constructor names the instance, as in `Choice[u8].Value(7)`. A pattern names
only the enum, as in `case Choice.Value(value)`: the matched value already
fixes `T`, so a pattern takes no type arguments.
