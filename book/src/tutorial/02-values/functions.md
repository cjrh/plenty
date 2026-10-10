# Define functions with clear interfaces

Every parameter and return type must be declared:

```plenty
def double(value: i64) -> i64:
    """Return twice the supplied value."""
    value * 2

def main() -> Result[(), IoError]:
    print(double(21))?
    Ok(())
```

```output
42
```

`value: i64` declares the parameter type; `-> i64` declares the return type.
The final expression supplies the result, or you can write an explicit
`return value * 2`. Parameters are immutable.

Indent statements in the same block equally, using spaces. Tabs are rejected.

A string at the beginning of a function is its documentation. Triple quotes
allow a docstring to span lines. When a function should immediately return a
string, write `return "text"` so it cannot be mistaken for documentation.

Functions are known throughout the file, so one function may call a function
declared later. Calls currently use positional arguments only.
