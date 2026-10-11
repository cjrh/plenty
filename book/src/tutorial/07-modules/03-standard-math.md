# Use the standard math library

The compiler includes `std.math`. Import it like any other module; no companion
source file or download is needed. Its floating-point API accepts `f64`.

```plenty
import std.math as math

def main() -> Result[(), Failure]:
    print(math.sqrt(81.0)?)?
    print(math.factorial(5)?)?
    print(math.comb(5, 2)?)?
    print(math.isclose(math.radians(180.0), f64.PI, 1e-9, 0.0)?)?
    print(math.sqrt(-1.0) == Result[f64, math.MathError].Err(math.MathError.Domain))?
    values = [2, 3, 4]?
    print(math.sum(&values, 0))?
    print(math.prod(&values, 1))?
    Ok(())
```

```output
9.0
120
10
True
True
9
24
```

`?` propagates mathematical errors just like I/O errors. Match on a result when
you want to recover from `math.MathError.Domain` or `math.MathError.Overflow`.
Integer combinatorics return `u64`, reporting overflow instead of wrapping.
`floor`, `ceil`, and `trunc` return checked `i64` results.

`isclose` takes both tolerances explicitly. Constants use `f64.PI`, `f64.E`,
`f64.TAU`, `f64.INF`, and `f64.NAN`. Low-level numeric methods such as
`x.sqrt()` follow IEEE arithmetic and may return NaN; the imported functions
provide recoverable domain errors.

See the [math reference](../../design/facilities/math.md) for available functions
and their fixed-width and numerical limits.
