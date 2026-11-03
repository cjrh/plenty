# Units of measure: spike evidence

Spikes 1–8 ran on the release compiler built from `main` on 2026-11-02, before
issue #701, and were rerun on `eb772202` (after issue #701) with identical
output except spike 5. They support the
[units of measure proposal](../units-of-measure.md). The
[built-in quantity](#built-in-quantity-after-issue-701) section shows the same
cases with `Quantity[T, D]`. Outputs are exact except that error paths are
shortened to the file name.

## Spike 1: Same-dimension arithmetic

Phantom tags give distinct types; `@newtype` forwards `+`, `-`, `<` and unary `-`.

```text
# Phantom-tagged generic newtype: same-dimension arithmetic, distinct types.
class Length:
    pass
class Time:
    pass

@copy
@newtype
class Q[T, D]:
    value: T

def main() -> Result[(), IoError]:
    a = Q[f64, Length](3.0)
    b = Q[f64, Length](4.5)
    print(a + b, a - b, a < b, -a)?
    t = Q[f64, Time](2.0)
    print(t)?
    Ok(())
```

Output:

```text
Q[f64, Length](value=7.5) Q[f64, Length](value=-1.5) True Q[f64, Length](value=-3.0)
Q[f64, Time](value=2.0)
```

## Spike 2: Mixed dimensions are rejected

The forwarding rule already rejects `Length + Time`.

```text
class Length:
    pass
class Time:
    pass
@copy
@newtype
class Q[T, D]:
    value: T
def main() -> Result[(), IoError]:
    a = Q[f64, Length](3.0)
    t = Q[f64, Time](2.0)
    print(a + t)?
    Ok(())
```

Output:

```text
error: s2.plenty:12:11: `@newtype` class `Q[f64, Length]` forwards `+` only between two `Q[f64, Length]` values, not `Q[f64, Length]` and `Q[f64, Time]`; read the field, or declare an operator hook for these operands
```

## Spike 3: Domain storage loses the unit on `-`

`Positive - Positive` returns `i64`, so `d` is a bare `i64`; the annotation `n: i64 = d` compiles.

```text
# Domain storage through a newtype: does Positive survive + and -?
class Length:
    pass
@copy
@newtype
class Q[T, D]:
    value: T
def main() -> Result[(), IoError]:
    a = Q[Positive[i64], Length](3)
    b = Q[Positive[i64], Length](4)
    s = a + b
    d = a - b
    print(s, d)?
    n: i64 = d
    Ok(())
```

Output:

```text
Q[Positive[i64], Length](value=7) -1
```

## Spike 4: A generic operator hook is accepted when unused

The restriction is checked only at a use.

```text
# Generic operator hook: can __mul__ take a different dimension?
class Q[D]:
    value: f64
    def __mul__[E](self, other: &Q[E]) -> f64:
        self.value * other.value
def main() -> () :
    ()
```

Output:

```text
(compiles; no output)
```

## Spike 4b: A generic operator hook is rejected when used

`Length * Time` cannot be written with one generic `__mul__`.

```text
class L:
    pass
class T:
    pass
class Q[D]:
    value: f64
    def __mul__[E](self, other: &Q[E]) -> f64:
        self.value * other.value
def main() -> Result[(), IoError]:
    a = Q[L](2.0)
    b = Q[T](3.0)
    print(a * b)?
    Ok(())
```

Output:

```text
error: s4b.plenty:7:5: operator hooks cannot have method type parameters, defaults, or yield
```

## Spike 5: No float domain

`Positive[f64]` is rejected. On 2026-11-02 the error also suggested `NonZero[f64]`, itself rejected; issue #700 removed that hint (local PR #883).

```text
# Positive over floats?
def f(x: Positive[f64]) -> f64:
    x.get()
def main() -> ():
    ()
```

Output:

```text
error: s5.plenty:2:10: `Positive` requires a signed integer type
```

## Spike 5b: `NonZero` is integer-only

```text
def f(x: NonZero[f64]) -> f64:
    x.get()
def main() -> Result[(), IoError]:
    print(f(2.0))?
    Ok(())
```

Output:

```text
error: s5b.plenty:1:10: f64 does not satisfy IntType
```

## Spike 6: Hand-written classes per dimension

Works, but each class has one `__mul__` signature, so `Metres * Metres` and `Metres * f64` cannot both use it; the scalar goes through `__rmul__`.

```text
# One hook signature: scalar via __rmul__, quantity*quantity via __mul__.
@copy
class Metres:
    value: f64
    def __mul__(self, other: &Metres) -> SquareMetres:
        SquareMetres(self.value * other.value)
    def __rmul__(self, k: f64) -> Metres:
        Metres(k * self.value)
    def __truediv__(self, other: &Seconds) -> MetresPerSecond:
        MetresPerSecond(self.value / other.value)
@copy
class Seconds:
    value: f64
@copy
class SquareMetres:
    value: f64
@copy
class MetresPerSecond:
    value: f64
def main() -> Result[(), IoError]:
    m = Metres(3.0)
    print(m * m, 2.0 * m, m / Seconds(1.5))?
    Ok(())
```

Output:

```text
SquareMetres(value=9.0) Metres(value=6.0) MetresPerSecond(value=2.0)
```

## Spike 7: Code generic over one dimension

Additive unit-polymorphic code works today.

```text
# Unit-polymorphic code over a phantom dimension, today.
class Length:
    pass
@copy
@newtype
class Q[T, D]:
    value: T
def total[T, D](xs: &list[Q[T, D]]) -> Q[T, D]:
    mut sum = xs[0]
    for i in range(1, len(xs)):
        sum = sum + xs[i]
    sum
def main() -> Result[(), AllocError | IoError]:
    xs = [Q[f64, Length](1.5), Q[f64, Length](2.0)]?
    print(total(&xs))?
    Ok(())
```

Output:

```text
Q[f64, Length](value=3.5)
```

## Spike 8: `NonZero` storage

`*` keeps `NonZero`; `+` returns a bare `i64` and drops the unit.

```text
# Positive survives + inside a newtype; NonZero divisor through a newtype.
class Count:
    pass
@copy
@newtype
class Q[T, D]:
    value: T
def main() -> Result[(), IoError]:
    a = Q[NonZero[i64], Count](3)
    b = Q[NonZero[i64], Count](-2)
    print(a * b, a + b)?
    Ok(())
```

Output:

```text
Q[NonZero[i64], Count](value=-6) 1
```

## Built-in quantity after issue #701

Run on `eb772202`. Spike 4b's product and spike 3's subtraction now work, and
the proposal's opening example, with local unit constants in place of
`std.units`, prints the output the proposal gives.

```text
dimension Length("m")
dimension Time("s")
dimension Velocity = Length / Time
const metre: Quantity[f64, Length] = Quantity[f64, Length](1.0)
const second: Quantity[f64, Time] = Quantity[f64, Time](1.0)
const km: Quantity[f64, Length] = 1000.0 * metre
const minute: Quantity[f64, Time] = 60.0 * second
const hour: Quantity[f64, Time] = 3600.0 * second

def speed(d: Quantity[f64, Length], t: Quantity[f64, Time]) -> Quantity[f64, Velocity]:
    d / t

def main() -> Result[(), IoError]:
    d = 3.2 * km
    t = 41.0 * minute
    v = speed(d, t)
    print(v, v.to(km / hour))?
    a = Quantity[f64, Length](2.0)
    b = Quantity[f64, Time](3.0)
    print(a * b)?
    p = Quantity[Positive[i64], Length](5)
    print(p - p, p * p)?
    Ok(())
```

Output:

```text
1.3008130081300813 m/s 4.682926829268292
6.0 m*s
0 m 25 m^2
```

Mixed dimensions:

```text
dimension Length("m")
dimension Time("s")
def main() -> Result[(), IoError]:
    d = Quantity[f64, Length](1.0)
    t = Quantity[f64, Time](1.0)
    print(d + t)?
    Ok(())
```

```text
error: q2.plenty:6:11: quantity `+` requires equal dimensions, not `Length` and `Time`
```

`p - p` keeps the unit and drops the domain:

```text
dimension Length("m")
def main() -> Result[(), IoError]:
    p = Quantity[Positive[i64], Length](5)
    q: Quantity[Positive[i64], Length] = p - p
    print(q)?
    Ok(())
```

```text
error: q3.plenty:4:5: expected Quantity[Positive[i64], Length], got Quantity[i64, Length]
```
