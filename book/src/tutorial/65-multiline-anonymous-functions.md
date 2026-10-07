# Multiline anonymous functions

Use `def` without a name to create a function value. Its indented body can have
local bindings, loops, conditionals, early returns, and `?`, just like a named
function. Every parameter and the return value still need explicit types.

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

This function has type `Callable[[i64], i64]`. Creating it does not allocate.
You can return it from another function, store it, and pass it to a typed callback
parameter. In a generic function, its annotations can use the enclosing type
parameters.

The implemented form cannot capture surrounding local bindings. Pass those values
as explicit parameters. Named module functions and imported symbols remain
available normally. Bind the anonymous function before passing it to another
call: indented anonymous bodies inside parentheses are not supported.
