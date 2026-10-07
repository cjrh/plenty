# 3. Choose the width of an integer

The built-in integer names specify their sizes:

| Types | Meaning |
| --- | --- |
| `i8`, `i16`, `i32`, `i64` | Signed integers of 8, 16, 32, or 64 bits |
| `u8`, `u16`, `u32`, `u64` | Unsigned integers of those widths |

For example, `i8` can hold -128 through 127, and `u8` can hold 0 through 255.
Floating-point numbers use `f32` or `f64`; other primitive value types include
`bool` and `str`.

A whole-number literal without a suffix, such as `42`, defaults to `i64` when
there is no annotation, parameter, return type, or typed arithmetic context.
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

Arithmetic overflow is a runtime error, rather than wrapping silently. Explicit
integer casts have different semantics: narrowing discards high bits. For
example, `u8(257)` produces `1`. Widening preserves signedness appropriately;
a same-width cast between signed and unsigned types reinterprets the bits.
Use casts deliberately; they are not range-validation functions.

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

## Work with floating-point numbers

A decimal or exponent literal defaults to `f64`. Use an `f32` suffix when you
want 32-bit arithmetic. Both operands must have the same type; casts are explicit:

```plenty
def main() -> Result[(), IoError]:
    distance: f64 = 7.5
    time: f64 = 2.0
    print(distance / time)?
    print(1.25f32 + 0.5f32)?
    print(2.5e-2)?
    print(f64(3) / 2.0)?
    print(i32(-2.75))?
    Ok(())
```

```output
3.75
1.75
0.025
1.5
-2
```

Float-to-integer casts discard the fractional part toward zero and clamp results
outside the integer's range; NaN converts to zero. Integer-to-float casts and
`f64` to `f32` casts may round. There is no implicit widening or narrowing:

```plenty-error
def main() -> ():
    small: f32 = 1.5f64
```

```error
expected f32, got f64
```

Write `1.5f32` or `f32(1.5)` instead. Floats follow IEEE arithmetic: division by
zero can produce infinity or NaN, and arithmetic overflow can produce infinity.
NaN is unequal to everything, including itself; ordered comparisons with it are
false. The same comparison rules apply inside collections and enum payloads.

```plenty
def main() -> Result[(), IoError]:
    zero = 0.0
    print(1.0 / zero)?
    unknown = zero / zero
    print(unknown == unknown)?
    print(unknown != unknown)?
    Ok(())
```

```output
inf
False
True
```

Floats support `+`, `-`, `*`, `/`, unary signs, and comparisons.
`//` and `%` currently require integers. Floats can be list elements, dictionary
values, class fields, and enum payloads, but cannot be dictionary keys or set
elements. Float literals that overflow their declared width are compile errors.
