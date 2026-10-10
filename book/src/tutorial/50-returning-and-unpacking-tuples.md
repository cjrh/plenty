# Returning and unpacking tuples

A tuple groups values with different types. Write `(value,)` for one component;
parentheses without a comma simply group an expression. Function signatures may
spell a tuple as `(i64, str)` or `tuple[i64, str]`. Unpacking transfers owned
components, and `_` discards a component. Indices must be integer literals.

```plenty
def measurement() -> (i64, str):
    (42, "cm")

def main() -> Result[(), Failure]:
    value, unit = measurement()
    print(value)?
    print(unit)?
    for number, word in [(1, "one"), (2, "two")]?:
        print((number, word))?
    print((3, "three"))?
    Ok(())
```
```output
42
cm
(1, "one")
(2, "two")
(3, "three")
```

A tuple stores its components inline, so `(a, b)` never allocates and has type
`tuple[A, B]` directly. The list above still allocates, so its display keeps
its `?`.
