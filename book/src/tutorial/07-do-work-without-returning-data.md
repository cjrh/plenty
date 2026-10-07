# 7. Do work without returning data

The unit type, written `()`, describes completion without producing data.
It is not a missing-value marker. A fallible function with no success data
returns `Result[(), E]`:

```plenty
def greet(name: str) -> Result[(), Failure]:
    print(("Hello, " + name)?)?
    Ok(())

def greet_if(enabled: bool, name: str) -> Result[(), Failure]:
    if not enabled:
        return Ok(())
    greet(name)

def main() -> Result[(), Failure]:
    greet_if(False, "Ada")?
    greet_if(True, "Ada")?
    Ok(())
```

```output
Hello, Ada
```

A bare `return` returns unit in a function declared `-> ()`. `print` returns
unit inside a Result, so `print(value)?` has a unit success value. Use `pass`
for an intentionally empty block. Unit can be a return type, but unit parameters
and stored unit bindings are not supported yet.

There is no `None` value and no implicit nullable type. Lesson 19 introduces
`Option` for absence and `Result` for recoverable errors.
