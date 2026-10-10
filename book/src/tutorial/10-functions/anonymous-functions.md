# Multiline anonymous functions

Use `def` without a name to create a function value. Its body follows the same
rules as a named function; parameters and the return value need explicit types.

```plenty
def main() -> Result[(), Failure]:
    transform = def(value: i64) -> i64:
        adjusted = value * 2
        if adjusted > 10:
            return 10
        adjusted + 1
    print(transform(3))?
    print(transform(8))?
    Ok(())
```
```output
7
10
```

This function has type `Callable[[i64], i64]` and can be stored, returned, or
passed to a callback parameter.

Bind the anonymous function before passing it to another call: indented bodies
inside parentheses are not supported.
