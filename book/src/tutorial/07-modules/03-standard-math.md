# Use the standard math library

The compiler includes `std.math`. Import it like any other module; no companion
source file or download is needed. Its floating-point API accepts `f64`.

```plenty
import std.math as math
from std.math import pi

const half_turn: f64 = f64.PI

def main() -> Result[(), Failure]:
    print(math.sqrt(81.0)?)?
    print(math.factorial(5)?)?
    print(math.comb(5, 2)?)?
    print(math.isclose(math.radians(180.0), pi, 1e-9, 0.0)?)?
    print(pi == half_turn)?
    print(math.sqrt(-1.0) == Result[f64, math.MathError].Err(math.MathError.Domain))?
    values = [2, 3, 4]?
    print(math.sum(&values, 0))?
    print(math.prod(&values, 1))?
    print(math.min_all(&values).unwrap())?
    print(math.max_all(&values).unwrap())?
    print(math.round(2.5)?)?
    print(math.round(3.5)?)?
    quotient, remainder = math.divmod(-7, 3)?
    print(quotient)?
    print(remainder)?
    interval = range(1, 5)
    print(math.sum_iter(&interval, 0))?
    Ok(())
```

```output
9.0
120
10
True
True
True
9
24
2
4
2
4
-3
2
10
```

`?` propagates mathematical errors just like I/O errors. Match on a result when
you want to recover from `math.MathError.Domain` or `math.MathError.Overflow`.
Integer combinatorics return `u64`, reporting overflow instead of wrapping.
`floor`, `ceil`, `trunc`, and `round` return checked `i64` results. `round`
chooses the even integer at halfway values. `divmod` returns an integer floor
quotient and remainder with the divisor's sign; use `divmod_float` for floats.
Division by zero is a recoverable `Domain` error.

`min_all` and `max_all` borrow a list and return `Nothing` for an empty list.
They retain the first value on ties. `sum_iter` and `prod_iter` also borrow
ranges, sets, and dictionary keys; their explicit start supplies the numeric
type and the result for an empty input. The original collections remain usable.

`isclose` takes both tolerances explicitly. Module constants are `math.pi`,
`math.e`, `math.tau`, `math.inf`, and `math.nan`, all `f64`. Import one as a
value, as the example does with `pi`. The type constants `f64.PI`, `f64.E`,
`f64.TAU`, `f64.INF`, and `f64.NAN` also remain available. Low-level methods such as
`x.sqrt()` follow IEEE arithmetic and may return NaN; the imported functions
provide recoverable domain errors.

Declare your own immutable numeric constants with `const name: T = value`, as
with `half_turn`. Initializers accept numeric literals or numeric type constants.
Use `pub const` to export one from a module.

See the [math reference](../../design/stdlib/math.md) for available functions
and their fixed-width and numerical limits. The
[standard library reference](../../design/stdlib/index.md) lists every module.
