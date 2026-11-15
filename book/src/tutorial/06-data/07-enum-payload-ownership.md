# Creating enum values

Payload arguments follow the ordinary [copy or move rule](../../design/language/05-ownership/01-bindings-moves-and-copies.md):
copyable values such as `str` copy into the enum, while affine values such as
lists transfer ownership. The enum's own `@copy` annotation controls later
uses of the constructed value. Constructing it adds no allocation:

```plenty
@copy
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
