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

`is_ok()` and `is_err()` answer whether a result succeeded without looking at
its payload. They borrow the result, so it can still be matched or returned.

```plenty
def divide(left: i64, right: i64) -> Result[i64, str]:
    if right == 0:
        return Err("division by zero")
    Ok(left // right)

def main() -> Result[(), IoError]:
    outcome = divide(8, 0)
    print(outcome.is_ok(), outcome.is_err())?
    print(divide(8, 2).is_ok())?
    match outcome:
        case Ok(value):
            print(value)?
        case Err(message):
            print(message)?
    Ok(())
```

```output
False True
True
division by zero
```
