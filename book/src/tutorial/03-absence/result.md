# Represent a recoverable error

Use `Result[T, E]` when failure has an explanation. `Ok(value)` carries
success data of type `T`; `Err(error)` carries a cause of type `E`.

```plenty
def divide(left: i64, right: i64) -> Result[i64, str]:
    if right == 0:
        return Err("division by zero")
    Ok(left // right)

def show(left: i64, right: i64) -> Result[(), IoError]:
    match divide(left, right):
        case Ok(value):
            print(value)?
        case Err(message):
            print(message)?
    Ok(())

def main() -> Result[(), IoError]:
    show(8, 2)?
    show(8, 0)?
    Ok(())
```

```output
4
division by zero
```

The caller can recover and continue, as `show` does here. A result does not
implicitly unwrap itself or raise an exception.

When success has no data, use unit as the success type:

```plenty
def validate(name: str) -> Result[(), str]:
    if len(name) == 0:
        return Err("name is empty")
    Ok(())

def show(name: str) -> Result[(), IoError]:
    match validate(name):
        case Ok(_):
            print("valid")?
        case Err(message):
            print(message)?
    Ok(())

def main() -> Result[(), IoError]:
    show("Plenty")?
    show("")?
    Ok(())
```

```output
valid
name is empty
```

The pattern `_` ignores a payload you do not need. `Ok(())` represents success
without data; `Nothing` represents absence.
