# Creating generators

An ordinary function can create and return a generator:

```plenty
def numbers() -> Generator[i64]:
    yield 10
    yield 20

def source() -> Generator[i64]:
    numbers()

def total() -> i64:
    values = source()
    mut result = 0
    for n in values:
        result = result + n
    result

def main() -> Result[(), IoError]:
    print(total())?
    Ok(())
```
```output
30
```

A factory that can fail can return
`Result[Generator[T], E]`. Write `Ok(numbers())` for success and use `?` at the
factory's call site. Wrapping a generator adds no allocation; dropping the
wrapper releases its captures without resuming the body.
