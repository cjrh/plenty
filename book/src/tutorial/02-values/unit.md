# Complete work without returning data

A function can complete without producing data. Its return type is the unit
type, `()`. This is different from a missing value.

```plenty
def show_if(enabled: bool, value: i64) -> Result[(), IoError]:
    if not enabled:
        return Ok(())
    print(value)?
    Ok(())

def main() -> Result[(), IoError]:
    show_if(False, 10)?
    show_if(True, 42)?
    Ok(())
```

```output
42
```

`Result[(), IoError]` combines unit success with a possible output error.
`print(value)?` therefore completes without supplying data.

In a function declared `-> ()`, a bare `return` returns unit. Use `pass`
for an intentionally empty block. Unit parameters and standalone unit bindings
are not supported. Use `Option` for absence; there is no `None` value.
