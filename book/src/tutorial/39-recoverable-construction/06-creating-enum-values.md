# Creating enum values

Enum variants also offer explicit fallible construction:

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
Result[Message, AllocError].Ok(Message.Text("hello"))
Result[Message, AllocError].Ok(Message.Empty)
```

Payload arguments move into the constructor and are dropped if its allocation
fails. Nullary variants take no arguments to `new`.
