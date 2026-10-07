# Creating generators

Generators can be returned inside `Result` and extracted with `?`:

```plenty
def numbers() -> Generator[i64]:
    yield 10
    yield 20

def source() -> Result[Generator[i64], AllocError]:
    numbers()

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

Call a generator function like any other function: `numbers()` returns
`Result[Generator[i64], AllocError]`. Creating the generator allocates its frame
but does not execute its body. Use `numbers()?` to propagate an allocation failure.
Argument expressions retain their own failure behavior, and moved arguments are
dropped on allocation failure.
Dropping a wrapped generator releases its captures without executing its body.
