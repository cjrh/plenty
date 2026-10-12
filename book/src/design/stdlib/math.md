# `std.math`

`std.math` provides floating-point functions with recoverable domain and
overflow errors, checked rounding to integers, integer algorithms, generic
numeric helpers, and constants. Its source is `stdlib/std/math.plenty`.
[Importing, the reserved `std` namespace, and compilation](index.md) are common
to every standard module.

```python
import std.math as math
from std.math import pi, MathError
```

Recoverable failures are `MathError.Domain` and `MathError.Overflow`. No
function in this module allocates.

## Floating-point functions

Inputs are `f64`, following Python's double-precision math surface. Explicitly
convert integers and `f32` inputs. Angles use radians.

| Functions | Result |
|---|---|
| `sqrt(x)`, `cbrt(x)` | `Result[f64, MathError]` |
| `exp(x)`, `exp2(x)`, `expm1(x)` | `Result[f64, MathError]` |
| `log(x)`, `log2(x)`, `log10(x)`, `log1p(x)`, `log_base(x, base)` | `Result[f64, MathError]` |
| `sin(x)`, `cos(x)`, `tan(x)`, `asin(x)`, `acos(x)`, `atan(x)`, `atan2(y, x)` | `Result[f64, MathError]` |
| `sinh(x)`, `cosh(x)`, `tanh(x)`, `asinh(x)`, `acosh(x)`, `atanh(x)` | `Result[f64, MathError]` |
| `pow(x, y)`, `fmod(x, y)` | `Result[f64, MathError]` |
| `floor(x)`, `ceil(x)`, `trunc(x)`, `round(x)` | `Result[i64, MathError]` |
| `divmod_float(a, b)` | `Result[(f64, f64), MathError]` |
| `fabs(x)`, `copysign(x, y)`, `degrees(x)`, `radians(x)` | `f64` |
| `isnan(x)`, `isinf(x)`, `isfinite(x)` | `bool` |
| `isclose(a, b, rel_tol, abs_tol)` | `Result[bool, MathError]` |

`MathError.Domain` reports invalid real operations, including negative square
roots, nonpositive logarithms, infinite trigonometric inputs, and zero finite
negative powers. `MathError.Overflow` reports finite inputs whose result
exceeds the floating or integer result range. Underflow succeeds. NaNs
propagate through floating results; rounding NaN is a domain error. Rounding
infinity or a value outside `[-2**63, 2**63)` overflows. Floating primitives
preserve signed zero; `fabs` clears its sign and `copysign` observes it.
Predicates and angular conversions have no recoverable failure. Angular
conversions follow IEEE arithmetic, including overflow to infinity. Integer
`.pow` remains a distinct, exact, trapping operation.

`round` rounds to the nearest integer, choosing the even integer at a halfway
value: `round(2.5)` is `Ok(2)` and `round(3.5)` is `Ok(4)`. It takes no decimal
places argument. The numeric `.round()` method keeps the float type and signed
zero; the library function checks the rounded value before converting to `i64`.

`divmod_float` returns the floor quotient and a remainder with the divisor's
sign, deriving both from the same floating remainder to avoid inconsistent
results near integral quotients. Zero remainders retain the divisor's sign;
zero quotients retain the sign of `a / b`. A zero divisor or any non-finite
input is `Domain`; a quotient beyond the finite float range is `Overflow`.
Floating rounding means `a == q * b + r` need not hold exactly.

`isclose` follows Python's weak symmetric comparison: exact equality succeeds
first, unequal infinities fail, and otherwise either relative tolerance or the
absolute tolerance may accept the difference. Negative tolerances produce
`Domain`; NaNs are never close. Both tolerances are explicit because default
and keyword arguments are unimplemented. `1e-9, 0.0` matches Python's defaults.

The module exports `pi`, `e`, `tau`, `inf`, and `nan` as immutable `f64`
[module constants](../language/02-source/modules.md#numeric-constants). They can also be imported
individually. Numeric types retain `f64.PI`, `f64.E`, `f64.TAU`, `f64.INF`, and
`f64.NAN`, with the same names on `f32`.

## Integer algorithms and generic helpers

| Function | Contract |
|---|---|
| `factorial(n: i64) -> Result[u64, MathError]` | Negative inputs are `Domain`; inputs above 20 overflow |
| `gcd(a: u64, b: u64) -> u64` | Euclidean algorithm; `gcd(0, 0)` is zero |
| `lcm(a: u64, b: u64) -> Result[u64, MathError]` | Either zero input gives zero; checked multiplication |
| `isqrt(n: u64) -> u64` | Exact floor square root over the entire `u64` range |
| `perm(n: u64, k: u64) -> Result[u64, MathError]` | Ordered selections; `k > n` gives zero |
| `comb(n: u64, k: u64) -> Result[u64, MathError]` | Unordered selections; `k > n` gives zero; cancels factors before multiplication |
| `abs[T](x: T) -> T` | Numeric magnitude; normalizes floating zero; negating a signed minimum traps |
| `min[T](a: T, b: T) -> T`, `max[T](a: T, b: T) -> T` | Compare two owned comparable values; ties retain the first |
| `min_all[T](items: &list[T]) -> Option[T]`, `max_all[T](items: &list[T]) -> Option[T]` | Copy the first extremum; `Nothing` for an empty list; elements must be copyable and comparable |
| `sum[T](items: &list[T], start: T) -> T`, `prod[T](items: &list[T], start: T) -> T` | Sequential numeric fold; empty lists return the explicit start |
| `sum_iter[I, T](items: &I, start: T) -> T`, `prod_iter[I, T](items: &I, start: T) -> T` | Borrowed list, set, range, or dictionary-key fold; numeric elements must match the start type |
| `divmod[T: IntType](a: T, b: T) -> Result[(T, T), MathError]` | Same-type integer floor quotient and remainder; zero divisor is `Domain`, signed minimum divided by `-1` is `Overflow` |

Integer combinatorics report `Overflow` when the result cannot fit `u64`.
Unlike Python, Plenty has no arbitrary-precision integer type. `gcd` and `lcm`
take two unsigned inputs; convert signed magnitudes explicitly. Generic helpers
specialize against operations in their bodies, with normal language errors for
unsupported types. They allocate nothing and retain ordinary arithmetic
overflow traps. Floating `sum` uses sequential addition, with no compensated
summation guarantee.

Collection helpers borrow their inputs and preserve them. Extrema use ordinary
comparisons, retaining the first value on ties (including signed zeros); NaNs
follow those comparisons, with no total-order or NaN-skipping policy. Dictionary
folds visit keys in insertion order; set order is unspecified. The fold start is
always explicit and determines the result type. Generators and custom iterator
protocols are not borrowed iterables; strings have nonnumeric elements.

## Runtime boundary and current limits

Only numeric methods and constants are compiler/runtime primitives. Platform
transcendentals, cancellation-sensitive `expm1`/`log1p`, and IEEE sign operations
need accurate scalar operations that ordinary arithmetic cannot substitute for.
The existing numeric-method boundary exposes these to `f32`, `f64`, and generic
`FloatType` functions, so user libraries can use them too. Algorithms, predicates,
conversions, tolerance comparisons, guards, and error construction remain Plenty
source. Generic arithmetic and integer combinatorics need no runtime additions.

This first module is inspired by CPython's `Doc/library/math.rst` and
`Modules/mathmodule.c`, particularly its domain/overflow policy and `isclose`.
It is not the complete Python math API. Missing operations include `hypot`,
`dist`, `fsum`, `sumprod`, `frexp`, `ldexp`, `nextafter`, `ulp`, `remainder`,
`fma`, special functions, and newer pi-scaled trigonometry. Accurate summation
and bit-level float decomposition need separate contracts and tests; simple
arithmetic substitutes must not imply Python's numerical guarantees.
Default arguments, arbitrary constant expressions, and custom iterator protocols
remain language design questions. `%` remains integer-only; use `fmod` or
`divmod_float` for floats. Powers use `.pow()` or `math.pow`; `**` is rejected
with a diagnostic naming `.pow()`. Remaining scheduling belongs in the
[backlog](../../backlog.md).
