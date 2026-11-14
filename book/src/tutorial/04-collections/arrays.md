# Keep a fixed number of values inline

Use an array when the number of same-typed values is part of the type. Its
storage is inline, so constructing the array itself cannot fail allocation.
A list remains the usual choice when its length changes at runtime.

```plenty
def calibration() -> array[i64, 3]:
    [10, 20, 30]

def main() -> Result[(), IoError]:
    mut values = calibration()
    values[1] = 25
    print(len(values), values[-1])?
    for value in &values:
        print(*value)?
    Ok(())
```
```output
3 30
10
25
30
```

The return type tells the compiler that `[10, 20, 30]` is an array. An annotated
binding or an array parameter provides the same context. Without it, the
literal constructs an ordinary list and needs `?` for allocation failure.
An array literal must contain exactly the declared number of elements.

Use `array[i64, 4].filled(0)` for four copies of a value, or
`array[i64, 0].filled(0)` for an empty array. For distinct owned elements,
`from_fn` calls a reusable function once per index; `try_from_fn` stops at its
first `Result` error and cleans up elements already constructed. See
[fixed inline arrays](../../design/language/03-types/arrays.md) for generic
literal inference and storage limits.
