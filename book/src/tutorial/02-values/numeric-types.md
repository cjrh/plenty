# Choose the width of an integer

The built-in integer names specify their sizes:

| Types | Meaning |
| --- | --- |
| `i8`, `i16`, `i32`, `i64` | Signed integers of 8, 16, 32, or 64 bits |
| `u8`, `u16`, `u32`, `u64` | Unsigned integers of those widths |

For example, `i8` can hold -128 through 127, and `u8` can hold 0 through 255.

A whole-number literal without a suffix, such as `42`, defaults to `i64` when
its type is unconstrained. An annotation, parameter, return type, or arithmetic
operand can supply the type instead.
Append a built-in integer type to choose another width:

```plenty
def main() -> Result[(), IoError]:
    small: u8 = 200u8
    offset: i32 = -12i32
    large: u64 = 1_000_000u64
    print(small)?
    print(offset)?
    print(large)?
    Ok(())
```

```output
200
-12
1000000
```

An annotation guides an unsuffixed literal, so `small: u8 = 200` works.
It does not convert an already typed initializer: use an explicit cast for that.
