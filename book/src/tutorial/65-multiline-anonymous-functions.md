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

Use an explicit list to capture surrounding bindings:

```plenty
def main() -> Result[(), Failure]:
    offset = 4
    values = [10, 20]?
    adjust = def [offset, values](index: i64) -> i64:
        values[index] + offset
    print(adjust(0))?
    print(adjust(1))?
    Ok(())
```
```output
14
24
```

`values` moves into the closure; `offset` is a scalar and copies normally.
The closure owns its captures and drops them when it leaves scope. Calling it
borrows that environment, so both calls work without copying the list. Creating
the closure itself does not allocate. Its concrete environment type differs from
the code-only `Callable` type. Captures cannot be moved out of the body; borrow
them or explicitly `copy` when an independent owner is needed.

Named module functions and imported symbols remain
available normally. Bind the anonymous function before passing it to another
call: indented anonymous bodies inside parentheses are not supported.
