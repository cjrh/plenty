# Creating enum values

Payload arguments move into the enum. Constructing it adds no allocation:

```plenty
enum Message:
    Empty
    Text(str)

def main() -> Result[(), IoError]:
    print(Message.Text("hello"))?
    print(Message.Empty)?
    Ok(())
```
```output
Message.Text("hello")
Message.Empty
```

Allocations inside argument expressions, such as building a string, still
return their own `Result`.
