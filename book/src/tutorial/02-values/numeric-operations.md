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

A cast never fails, so it can discard data instead: narrowing discards high
bits, so `u8(300)` produces `44`. Widening sign-extends signed inputs and
zero-extends unsigned inputs; same-width signed/unsigned casts reinterpret the
bits. Casts do not validate a value's range. Cast only when the value is known
to fit or when discarding bits is the intent.

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

Integers also support bitwise operations and shifts. A shift count must be
nonnegative and smaller than the integer width. Signed right shifts preserve
the sign; unsigned right shifts fill with zero. Left shifts discard high bits.

```plenty
def main() -> Result[(), IoError]:
    bits = 5u8
    print(bits & 3)?
    print(bits | 2)?
    print(bits ^ 3)?
    print(~bits)?
    print(bits << 1)?
    print(bits >> 1)?
    print(255u8.wrapping_add(1))?
    print(0u8.wrapping_sub(1))?
    print(128u8.wrapping_mul(2))?
    Ok(())
```

```output
1
7
6
250
10
2
0
255
0
```

Use `wrapping_add`, `wrapping_sub`, or `wrapping_mul` when a calculation
deliberately wraps at the integer width, as in a hash or a random-number
generator. Ordinary arithmetic still traps on overflow. Parenthesize mixed
bitwise expressions when that makes the intended grouping easier to read.
