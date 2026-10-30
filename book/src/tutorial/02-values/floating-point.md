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

Float-to-integer casts discard the fractional part toward zero. The result must
fit the integer type: `u8(255.9)` is `255`, while a `300.7`, an infinity, or NaN
held in a variable stops the program with `error: integer cast out of range`.
Integer-to-float casts and `f64` to `f32` casts may round. There is no implicit widening or narrowing:

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

When a zero divisor is an error rather than a source of infinity,
`x.div_checked(y)` returns an `Err` holding a `ZeroDivisionError` for a zero `y` and
`Ok(x / y)` otherwise; see
[recover from overflow](numeric-operations.md#recover-from-overflow).

Floats support `+`, `-`, `*`, `/`, unary signs, and comparisons.

`//` and `%` currently require integers. Floats cannot be dictionary keys or set elements. Float literals that overflow their declared width are compile errors.

Math functions are methods on a float, and each float type provides the
constants `PI` and `E`:

```plenty
def main() -> Result[(), IoError]:
    radius = 2.0
    print(f64.PI * radius.pow(2.0))?
    print((9.0).sqrt())?
    print(f64.E.log())?
    print((0.0).cos())?
    print((2.5).floor())?
    print((2.5).ceil())?
    print((7.5).fmod(2.0))?
    Ok(())
```

```output
12.566370614359172
3.0
1.0
1.0
2.0
3.0
1.5
```

`log` is the natural logarithm, `sin` and `cos` take radians, and `exp` raises
e to a power. `floor` and `ceil` return floats; cast the result for an integer,
as in `i64(x.floor())`. `fmod` is the float remainder. These methods never stop
the program: `(-1.0).sqrt()` is NaN. An integer needs a cast first, as in
`f64(n).sqrt()`.
