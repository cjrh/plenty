# Make decisions that produce values

A condition must have type `bool`. Comparisons such as `==`, `!=`, `<`, `<=`,
`>`, and `>=` produce booleans. There is no implicit conversion of zero, an
empty string, or another value to `False`.

An `if` can supply a function's result:

```plenty
def maximum(first: i64, second: i64) -> i64:
    if first > second:
        first
    else:
        second

def main() -> Result[(), IoError]:
    print(maximum(7, 12))?
    Ok(())
```

```output
12
```

Both continuing branches need the same type. Use `elif` for additional cases.
For a short choice, Python's conditional expression is also supported:

```plenty
def main() -> Result[(), IoError]:
    age = 20
    category = "adult" if age >= 18 else "child"
    print(category)?
    Ok(())
```

```output
adult
```

`and` and `or` short-circuit: the right side is evaluated only when needed.
`not` negates a boolean. All three require boolean operands:

```plenty
def main() -> Result[(), IoError]:
    divisor = 0
    safe = divisor != 0 and 10 // divisor > 1
    print(safe)?
    print(not safe)?
    Ok(())
```

```output
False
True
```

The division is skipped here. Chained comparisons such as `0 < x < 10` are
not supported yet; write `0 < x and x < 10`.
