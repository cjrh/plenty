# Work with floating-point numbers

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
false.

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
`//` and `%` currently require integers. Floats cannot be dictionary keys or set elements. Float literals that overflow their declared width are compile errors.
