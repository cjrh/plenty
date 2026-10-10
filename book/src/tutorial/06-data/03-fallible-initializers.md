# Initialize fallible fields

A record stores its fields inline. Calling its constructor does not allocate
storage for the instance. Its initializer may still need fallible operations:
here the list field reserves capacity during construction.

An initializer returning `Result[(), AllocError]` makes `Class(...)` return
`Result[Class, AllocError]`:

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
