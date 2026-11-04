# Define functions with clear interfaces

Every parameter and return type must be declared:

```plenty
def double(value: i64) -> Result[i64, OverflowError[i64]]:
    """Return twice the supplied value."""
    Ok(value * 2)

def main() -> Result[(), IoError | OverflowError[i64]]:
    print(double(21)?)?
    Ok(())
```

```output
42
```

`value: i64` declares the parameter type. The return type
`Result[i64, OverflowError[i64]]` describes either a doubled integer or overflow.
`Ok(value * 2)` supplies success; the multiplication returns an error from
`double` if it cannot fit. You can also write `return Ok(value * 2)`.
The caller uses `double(21)?` to obtain the successful integer or pass the error
onward. Parameters are immutable. See [recoverable errors](../03-absence/result.md)
for handling either outcome explicitly.

Indent statements in the same block equally, using spaces. Tabs are rejected.

A string at the beginning of a function is its documentation. Triple quotes
allow a docstring to span lines. When a function should immediately return a
string, write `return "text"` so it cannot be mistaken for documentation.

Functions are known throughout the file, so one function may call a function
declared later. Name arguments when that makes their meaning clearer:

```plenty
def rectangle(width: i64, height: i64 = 2) -> Result[i64, OverflowError[i64]]:
    Ok(width * height)

def main() -> Result[(), IoError | OverflowError[i64]]:
    print(rectangle(height=3, width=4)?)?
    print(rectangle(4)?)?
    Ok(())
```

```output
12
8
```

Positional arguments come first. After the first named argument, all remaining
arguments must be named; their order need not match the declaration. Supplied
expressions run in the order you write them. Required parameters come before
parameters with defaults.

Defaults run afresh for each omitted argument. Mutable defaults never share
storage between calls:

```plenty
def count(values: &mut list[i64] = []?) -> Result[i64, AllocError]:
    values.append(7)?
    Ok(len(values))

def main() -> Result[(), AllocError | IoError]:
    print(count()?)?
    print(count()?)?
    mut saved = [1, 2]?
    print(count(values=&mut saved)?)?
    print(len(saved))?
    Ok(())
```

```output
1
1
3
3
```

An omitted borrowed argument gets temporary storage that lasts for the call.
Pass your own buffer to keep its changes. Defaults run after supplied arguments,
in declaration order; they cannot refer to another parameter. Allocation still
needs `?` and a compatible `Result` return type. If a default fails, that call
returns the error without entering the body. Callable values and builtins keep
positional arguments; splat unpacking is not supported.
