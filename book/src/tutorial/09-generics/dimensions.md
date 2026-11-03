# Calculate with quantities

Use `Quantity[T, D]` for a physical value whose dimension should be checked
in arithmetic. `T` is its numeric storage type and `D` is a compile-time
dimension. Declare bases with their coherent-unit symbols:

```plenty
dimension Length("m")
dimension Time("s")
dimension Velocity = Length / Time

const metre: Quantity[f64, Length] = Quantity[f64, Length](1.0)
const kilometre: Quantity[f64, Length] = 1000.0 * metre

def double[D: Dimension](value: Quantity[f64, D]) -> Quantity[f64, D]:
    value + value

def main() -> Result[(), IoError]:
    distance = 1500.0 * metre
    elapsed = Quantity[f64, Time](120.0)
    speed: Quantity[f64, Velocity] = distance / elapsed
    print(double(distance).to(kilometre), "km")?
    print(speed)?
    Ok(())
```
```output
3.0 km
12.5 m/s
```

`Length` and `Time` are distinct base dimensions. `Velocity` is an alias for
`Length / Time`. Dividing the distance by the elapsed time computes this
dimension, so the `speed` annotation checks successfully. Adding a time to a
length would be a compile error.

`double` infers its dimension parameter from its argument. The same function
works for a length, a time or a velocity. Dimension arguments add no runtime
fields. A dimension itself has no values: `length: Length` is invalid.

Unit constants can be shared across functions or exported from a module with
`pub const`. Their initializers are evaluated at compile time, including unit
scaling and arithmetic between constants.

The constructor takes the value in the coherent unit. A kilometre is a length
whose stored value is `1000.0`; `.to(kilometre)` divides by that value and
returns a plain `f64`. `.get()` reads the coherent value directly. Both methods
remove the dimension from the result, so keep quantities until the boundary
where you need a number. Printing a quantity includes its coherent-unit symbol.
The explicit `"km"` label accompanies the plain number returned by `.to(kilometre)`;
printing `double(distance)` directly would show `3000.0 m`.

Use an f-string to choose numeric precision: `f"{speed:.2f}"?` produces
`"12.50 m/s"`. As with other f-strings, include `AllocError` in the function's
error type when propagating its allocation failure.

Scalars only scale quantities. Write `0.0 * metre` for a zero length; `distance
+ 0.0` and `distance > 0.0` are errors. Dividing a length by a length produces
`Quantity[f64, One]`; use `.get()` to obtain its dimensionless number.

Dimension expressions also support products, integer powers and exact roots.
`Root[Pow[Length, 2], 2]` is `Length`; fractional dimensions are rejected.
In a generic function's parameter types, use concrete dimensions or bare
parameters. Expressions using parameters belong in return types or bodies.

See [dimensions](../../design/language/03-types/dimensions.md) for algebra and
visibility, and [quantities](../../design/language/03-types/quantities.md) for
integer/domain storage, conversions, checked arithmetic and current limits.
