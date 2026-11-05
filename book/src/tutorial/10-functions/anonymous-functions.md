# Anonymous functions

Use `def` without a name to create a function value. Parameters and the return
value need explicit types. Inside brackets, such as a call's arguments, the body
is one expression and its value is returned:

```plenty
def apply(f: Callable[[i64], Result[i64, OverflowError[i64]]], value: i64) -> Result[i64, OverflowError[i64]]:
    f(value)

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    print(apply(def(x: i64) -> Result[i64, OverflowError[i64]]: Ok(x * 2), 4)?)?
    print(apply(
        def(x: i64) -> Result[i64, OverflowError[i64]]: Ok(x + 1 if x > 0 else 0),
        4,
    )?)?
    Ok(())
```
```output
8
5
```

The body ends at the comma or closing bracket that ends the argument. Each
function has type `Callable[[i64], Result[i64, OverflowError[i64]]]` and can be
stored, returned, or passed to a callback parameter.

Outside brackets, the body is an indented block with the same rules as a named
function:

```plenty
def main() -> Result[(), IoError | OverflowError[i64]]:
    transform = def(value: i64) -> Result[i64, OverflowError[i64]]:
        adjusted = value * 2
        if adjusted > 10:
            return Ok(10)
        Ok(adjusted + 1)
    print(transform(3)?)?
    print(transform(8)?)?
    Ok(())
```
```output
7
10
```

The position decides the form. A function with statements cannot be written
inside brackets; give it an indented body and a name first, as `transform` has:

```plenty-error
def apply(f: Callable[[i64], Result[i64, OverflowError[i64]]], value: i64) -> Result[i64, OverflowError[i64]]:
    f(value)
def main() -> ():
    apply(def(x: i64) -> Result[i64, OverflowError[i64]]: return Ok(x * 2), 4)
```
```error
inside brackets an anonymous function body is one expression, and its value is returned; remove `return`
```

Parentheses are brackets, so a one-expression function can still be bound:
`double = (def(x: i64) -> Result[i64, OverflowError[i64]]: Ok(x * 2))`. Python's `lambda` is not available;
the compiler names the `def` spelling instead.
