# Creating enum values

Constructing an enum value never allocates, so it never needs recovery. The
payload is stored inline, and `Enum.Variant.new(...)` is the same as
`Enum.Variant(...)`:

```plenty
enum Message:
    Empty
    Text(str)

def main() -> Result[(), IoError]:
    print(Message.Text.new("hello"))?
    print(Message.Empty.new())?
    Ok(())
```
```output
Message.Text("hello")
Message.Empty
```

Payload arguments move into the value. Allocations inside argument
expressions, such as building the string, keep their own `Result`.
