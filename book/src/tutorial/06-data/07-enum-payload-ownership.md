# Creating enum values

Constructing an enum value never allocates, so it never needs recovery. The
payload is stored inline. Use the ordinary variant call for a payload and the
variant name itself when there is no payload:

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

Payload arguments move into the value. Allocations inside argument
expressions, such as building the string, keep their own `Result`.
