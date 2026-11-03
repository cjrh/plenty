# Label data with a compile-time dimension

A [generic class](generic-classes.md) can carry a dimension argument without
storing that dimension in each value. Use `D: Dimension` to require such an
argument:

```plenty
dimension Length("m")
dimension Time("s")
dimension Velocity = Length / Time

@copy
class Sample[D: Dimension]:
    value: f64

def read[D: Dimension](sample: Sample[D]) -> f64:
    sample.value

def main() -> Result[(), IoError]:
    distance = Sample[Length](12.0)
    speed: Sample[Velocity] = Sample[Length / Time](3.0)
    print(read(distance), read(speed))?
    Ok(())
```
```output
12.0 3.0
```

`Length` and `Time` are distinct base dimensions. Their strings record the
coherent-unit symbols. `Velocity` is an alias: `Length / Time` denotes exactly
the same dimension, so the `speed` annotation accepts that constructor.
`read` infers `D` from its argument and returns the ordinary stored number.

The dimension is a compile-time label; `Sample[Length]` has just its `f64`
field. It cannot be substituted for `Sample[Time]`. A dimension itself has no
values, so `length: Length` is invalid.

This example teaches dimension labels, not a units library. `Sample` has no
dimension-aware arithmetic or conversion methods. The built-in quantity type
is not implemented yet; extracting `.value` gives an ordinary number and loses
the label.

Dimension expressions also support products, integer powers and exact roots.
`Length / Length` is `One`, and `Root[Pow[Length, 2], 2]` is `Length`.
Fractional dimensions are rejected. In a generic function's parameter types,
use concrete dimensions or bare parameters; expressions using parameters belong
in return types or bodies. See the [dimension reference](../../design/language/03-types/dimensions.md)
for exponent limits, module visibility and diagnostics.
