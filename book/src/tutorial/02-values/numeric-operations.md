# Calculate with numbers

Arithmetic requires matching types:

```plenty-error
def main() -> ():
    small = 7u8
    print(small + 1i64).unwrap()
```

```error
expected u8, got i64
```

Choose a matching literal or explicitly convert a value:

```plenty
def main() -> Result[(), IoError]:
    small = 7u8
    print(small + 1u8)?
    print(i64(small) + 1)?
    Ok(())
```

```output
8
8
```

`i64(small)` is safe because every `u8` value fits in an `i64`. When the target
cannot hold every value of the source, use `try_from`. It checks the range and
returns a `Result`: `Ok` with the converted value, or
`Err(ParseError.OutOfRange)` when the value does not fit.

```plenty
def main() -> Result[(), IoError]:
    wide = 200
    small = u8.try_from(wide).unwrap()
    print(small + 1u8)?
    Ok(())
```

```output
201
```

`.unwrap()` takes the converted value and stops the program on an error, as it
would for a `wide` of `300`. [Absence and failure](../03-absence/index.md) shows
how to handle the error instead.

A cast to an integer type requires the value to fit. A literal that does not
fit is rejected when the program is compiled:

```plenty-error
def main() -> ():
    small = u8(300)
```

```error
300 is out of range for u8; u8.wrapping_from keeps the low bits
```

A value known only when the program runs is checked then. With `wide = 300`,
`u8(wide)` stops the program with `error: integer cast out of range`, as
arithmetic overflow does. The same applies to a negative value cast to an
unsigned type, such as `u64(-1)`, and to an unsigned value above the signed
maximum, such as `i8(200u8)`. Cast when the value is known to fit, and use
`try_from` when it might not.

When discarding the high bits is the intent, say so with `wrapping_from`, which
never fails:

```plenty
def main() -> Result[(), IoError]:
    wide = 300
    negative = -1
    print(u8.wrapping_from(wide))?
    print(u64.wrapping_from(negative))?
    print(i8.wrapping_from(200u8))?
    Ok(())
```

```output
44
18446744073709551615
-56
```

Integer arithmetic overflow is a runtime error. An unsigned type has no negative
values, so unary minus requires a signed integer or a float:

```plenty-error
def main() -> ():
    small = 7u8
    print(-small).unwrap()
```

```error
unary minus is not defined for unsigned type u8
```

Convert first: `-i16(small)` is `-7`.

`+`, `-`, and `*` have their usual arithmetic precedence. `//` divides integers
and rounds down, including for negative values:

```plenty
def main() -> Result[(), IoError]:
    print(1 + 2 * 3)?
    print((1 + 2) * 3)?
    print(-7 // 3)?
    Ok(())
```

```output
7
9
-3
```

Integer division by zero is a runtime error. Use `/` for floating-point division.
