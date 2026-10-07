# Creating generators

Generators can be returned inside `Result` and extracted with `?`:

```plenty
def numbers() -> Generator[i64]:
    yield 10
    yield 20

def source() -> Result[Generator[i64], AllocError]:
    numbers.new()

def total() -> Result[i64, AllocError]:
    mut values = source()?
    mut result = 0
    for n in values:
        result = result + n
    Ok(result)

def main() -> Result[(), IoError]:
    print(total())?
    Ok(())
```
```output
Result[i64, AllocError].Ok(30)
```

`numbers()` and its `numbers.new()` alias return `Result[Generator[T], AllocError]`.
Argument expressions retain
their own failure behavior, and moved arguments are dropped on allocation failure.
Dropping a wrapped generator releases its captures without executing its body.
