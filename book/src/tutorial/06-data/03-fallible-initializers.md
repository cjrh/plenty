# Initialize fallible fields

An initializer can return `Result[(), E]` when setting up its fields may fail.
The constructor then returns `Result[Class, E]`. Here, reserving list capacity
can fail to allocate:

```plenty
class Buffer:
    values: list[i64]

    def __init__(self: &mut Buffer, size: i64) -> Result[(), AllocError]:
        self.values = list[i64].with_capacity(size)?
        Ok(())

def main() -> Result[(), Failure]:
    print(Buffer(8)?)?
    Ok(())
```
```output
Buffer(values=[])
```

If initialization fails, the fields already initialized are dropped, but the
incomplete instance's `__del__` is skipped. Successful initialization must fill
every field and return `Ok(())`.
