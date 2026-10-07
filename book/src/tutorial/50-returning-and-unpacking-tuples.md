# Returning and unpacking tuples

A tuple groups values with different types. Write `(value,)` for one component;
parentheses without a comma simply group an expression. Function signatures may
spell a tuple as `(i64, str)` or `tuple[i64, str]`. Unpacking transfers owned
components, and `_` discards a component. Indices must be integer literals.

```plenty
def measurement() -> Result[(i64, str), AllocError]:
    (42, "cm")

def main() -> Result[(), Failure]:
    value, unit = measurement()?
    print(value)?
    print(unit)?
    for number, word in [(1, "one")?, (2, "two")?]?:
        print((number, word)?)?
    print((3, "three"))?
    Ok(())
```
```output
42
cm
(1, "one")
(2, "two")
Result[tuple[i64, str], AllocError].Ok((3, "three"))
```

Tuple storage currently allocates. `(a, b)` returns `Result[tuple[A, B], AllocError]`;
it consumes its evaluated components even on failure. As with fallible collection
displays, use checked operations separately inside component expressions.
