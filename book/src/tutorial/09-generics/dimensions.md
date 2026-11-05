# Calculate with quantities

Use `Quantity[T, D]` for a physical value whose dimension should be checked
in arithmetic. `T` is its numeric storage type and `D` is a compile-time
dimension. The [`std.units`](../../design/stdlib/units.md) module provides the
SI dimensions, units and constants:

```plenty
import std.units as u
from std.units import Velocity, m, km, s, minute

def double[D: Dimension](value: Quantity[f64, D]) -> Quantity[f64, D]:
    value + value

def main() -> Result[(), IoError]:
    distance = 1500.0 * m
    elapsed = 2.0 * minute
    speed: Quantity[f64, Velocity] = distance / elapsed
    print(double(distance).to(km), "km")?
    print(speed, speed.to(km / u.hour), "km/h")?
    Ok(())
```
```output
3.0 km
12.5 m/s 45.0 km/h
```

`u.Length` and `u.Time` are distinct base dimensions. `Velocity` is an alias
for `Length / Time`. Dividing the distance by the elapsed time computes this
dimension, so the `speed` annotation checks successfully. Adding a time to a
length would be a compile error. When a dimension error involves `Velocity`,
the diagnostic includes a note naming `Velocity` and its normal form,
`std.units.Length * Pow[std.units.Time, -1]`. This connects the normalized
dimension to the name used in your program.

Units are constants. `km` is a length whose stored value is `1000.0`, and
`2.0 * minute` stores `120.0` seconds. Multiplying a number by a unit
constructs a quantity, and dividing one unit by another, as in `km / u.hour`,
gives another unit.

`double` infers its dimension parameter from its argument. The same function
works for a length, a time or a velocity. Dimension arguments add no runtime
fields: a quantity uses its numeric storage type's native representation.
Construction and `get` add no numeric conversion, and float `to` performs the
same division as plain numbers. A dimension itself has no values:
`length: u.Length` is invalid.

`.to(km)` divides by the unit's stored value and returns a plain `f64`.
`.get()` reads the coherent value directly. Both methods remove the dimension from the result, so keep quantities until the boundary
where you need a number. Printing a quantity includes its coherent-unit symbol.
The explicit `"km"` label accompanies the plain number returned by `.to(km)`;
printing `double(distance)` directly would show `3000.0 m`.

Use an f-string to choose numeric precision: `f"{speed:.2f}"?` produces
`"12.50 m/s"`. As with other f-strings, include `AllocError` in the function's
error type when propagating its allocation failure.

Scalars only scale quantities. Write `0.0 * m` for a zero length; `distance
+ 0.0` and `distance > 0.0` are errors. Dividing a length by a length produces
`Quantity[f64, One]`; use `.get()` to obtain its dimensionless number.

`std.units` also has dimension-aware functions. `u.sqrt` of an area returns a
length, `u.sum` adds a list of quantities, and `u.sin` takes an angle such as
`30.0 * u.degree`. Temperatures in Celsius or Fahrenheit convert at the
boundary with `u.from_celsius` and `u.to_celsius`, because their zero is
offset.

## Declare your own dimensions

Declare a base dimension for a quantity outside SI, or to keep two kinds of
value apart that SI would give the same dimension. Give each base its
coherent-unit symbol:

```plenty
dimension Pixel("px")
dimension Point("pt")
dimension PixelDensity = Pixel / Point

const px: Quantity[f64, Pixel] = Quantity[f64, Pixel](1.0)
const pt: Quantity[f64, Point] = Quantity[f64, Point](1.0)
const retina: Quantity[f64, PixelDensity] = 2.0 * px / pt

def main() -> Result[(), IoError]:
    width = 320.0 * pt
    print(width * retina)?
    Ok(())
```
```output
640.0 px
```

Unit constants can be shared across functions or exported from a module with
`pub const`, as `std.units` does. Their initializers are evaluated at compile
time, including unit scaling and arithmetic between constants. The
constructor takes the value in the coherent unit.

Dimension expressions also support products, integer powers and exact roots.
`Root[Pow[Length, 2], 2]` is `Length`; fractional dimensions are rejected.
In a generic function's parameter types, use concrete dimensions or bare
parameters. Expressions using parameters belong in return types or bodies.

See [`std.units`](../../design/stdlib/units.md) for its units, constants and
functions, [dimensions](../../design/language/03-types/dimensions.md) for algebra and
visibility, and [quantities](../../design/language/03-types/quantities.md) for
integer/domain storage, conversions, checked arithmetic and current limits.
