# Checking stream capabilities

Use `readable()` and `writable()` to inspect an open file through a shared reference.
Closed files return an error; `closed` itself remains an infallible property.

```plenty
def demo() -> Result[(), IoError]:
    with open("capabilities.txt", "w")? as file:
        print(file.readable()?)?
        print(file.writable()?)?
    Ok(())
def main() -> Result[(), IoError]:
    demo()?
    Ok(())
```
```output
False
True
```
