# std.complex

`from std.complex import Complex` provides complex numbers with `f64` real and
imaginary parts. The module is ordinary embedded Plenty: arithmetic is written
with [operator hooks](../language/04-expressions/operators.md) and the
functions use the `f64` numeric methods. Nothing in it allocates or fails.

```plenty
from std.complex import Complex, from_polar
from std.fmt import FormatError

def main() -> Result[(), IoError | FormatError]:
    a = Complex(1.5, 2.0)
    b = Complex(-0.5, 4.0)
    print(f"{a + b} {a - b} {a * b} {-a}"?)?
    print(f"{a / b}"?)?
    print(f"{b.conj()} {Complex(3.0, 4.0).abs()} {Complex(3.0, 4.0).norm()}"?)?
    print(a == Complex(1.5, 2.0), a != b, a.re, a.im)?
    print(a)?
    Ok(())
```
```output
1.0+6.0i 2.0-2.0i -8.75+5.0i -1.5-2.0i
0.4461538461538462-0.4307692307692308i
-0.5-4.0i 5.0 25.0
True True 1.5 2.0
std.complex.Complex(re=1.5, im=2.0)
```

`Complex(re, im)` is the generated constructor, and `re` and `im` are public
fields. Like every class instance, a `Complex` is affine: it moves on
assignment and by-value argument passing. Operators and methods borrow their
operands, so `a + b` leaves both usable. Write `Complex(z.re, z.im)` for an
independent value. There are no in-place hooks; accumulate with
`total = total + term` on a `mut` binding.

## Operations

| Operation | Result | Contract |
|---|---|---|
| `a + b`, `a - b`, `a * b`, `a / b` | `Complex` | Arithmetic on two `Complex` operands |
| `-a` | `Complex` | Negate both parts |
| `a == b`, `a != b` | `bool` | Structural: both parts compare as `f64` |
| `z.scale(factor: f64)` | `Complex` | Multiply both parts by a real factor |
| `z.conj()` | `Complex` | Complex conjugate |
| `z.abs()` | `f64` | Magnitude, scaled like `hypot` |
| `z.norm()` | `f64` | Squared magnitude `re * re + im * im` |
| `z.arg()` | `f64` | Phase angle `im.atan2(re)`, in `[-pi, pi]` |
| `z.to_polar()` | `(f64, f64)` | `(z.abs(), z.arg())` |
| `from_polar(r: f64, theta: f64)` | `Complex` | `Complex(r * cos(theta), r * sin(theta))` |
| `z.exp()` | `Complex` | Complex exponential |
| `z.log()` | `Complex` | Principal natural logarithm: `abs().log() + arg() i` |
| `z.sqrt()` | `Complex` | Principal square root: nonnegative real part |
| `z.pow(exponent: &Complex)` | `Complex` | Principal value of `exp(exponent * log(z))` |

Both operands of an operator must be `Complex`; there are no mixed operations
with `f64`. Operator hooks dispatch on the left operand's type and cannot be
overloaded, so `z * 2.0` cannot share `__mul__` with `z * w`. Use
`z.scale(2.0)` for a real factor, or `Complex(2.0, 0.0)` as an operand.
`scale` is also the more exact choice for non-finite values: `Complex(inf, 1.0)
* Complex(2.0, 0.0)` computes `inf * 0.0` for its imaginary part and gives NaN,
while `scale(2.0)` gives `inf+2.0i`.

Multiplication uses the textbook formula. Division uses Smith's algorithm,
which scales by the larger divisor part so that `c * c + d * d` never overflows
or underflows. `abs` scales the same way, so `Complex(1e300, 1e300).abs()` is
finite. `norm` does not scale and overflows first.

```plenty
from std.complex import Complex, from_polar
import std.math as math
from std.fmt import FormatError

def main() -> Result[(), IoError | FormatError | math.MathError]:
    z = from_polar(2.0, 0.5)
    r, theta = z.to_polar()
    print(math.isclose(r, 2.0, 1e-15, 0.0)?, math.isclose(theta, 0.5, 1e-15, 0.0)?)?
    print(f"{Complex(-4.0, 0.0).sqrt()} {Complex(3.0, 4.0).sqrt()}"?)?
    print(f"{Complex(-1.0, 0.0).log()}"?)?
    i = Complex(0.0, 1.0)
    print(f"{i.pow(i)} {Complex(0.0, 0.0).pow(Complex(2.0, 0.0))}"?)?
    print(f"{Complex(0.0, f64.PI).exp()}"?)?
    Ok(())
```
```output
True True
0.0+2.0i 2.0+1.0i
0.0+3.141592653589793i
0.20787957635076193+0.0i 0.0+0.0i
-1.0+1.2246467991473532e-16i
```

The functions are computed from `f64` primitives, so results carry their
rounding: `exp(pi i)` has a tiny imaginary part, and `pow` inherits the
rounding of both `exp` and `log`. Compare results with a tolerance, for example
[`math.isclose`](math.md) on each part. `pow` returns `1+0i` for a zero
exponent, including a NaN base, as `f64.pow` does, and `0+0i` for a zero base
with a real positive exponent.

## Floating-point behavior

Complex arithmetic follows `f64` arithmetic: it never traps and never returns
`Result`. Overflow gives infinity and invalid operations give NaN, which then
propagates. This matches the `/` operator on `f64` and the numeric methods, and
lets operators compose. It differs from the [`std.math`](math.md) functions,
which report domain and overflow errors; check parts with `math.isfinite` when
a result must be finite.

Dividing by `Complex(0.0, 0.0)` divides each part of the numerator by the real
zero, exactly as `f64` division does: `(1+2i) / 0` is `inf+infi`, and `0 / 0` is
`NaN+NaNi`. Python raises `ZeroDivisionError` instead.

Signed zero is preserved where the formulas define it. `log` of zero is
`-inf` plus the zero's `arg`, and `sqrt` of a signed zero keeps the sign of the
imaginary part. `Complex(-1.0, 0.0).arg()` is `pi` and `Complex(-1.0, -0.0).arg()`
is `-pi`, so the branch cut of `log`, `sqrt` and `pow` along the negative real
axis follows the sign of the imaginary zero. `abs` returns infinity when either
part is infinite, even if the other is NaN.

```plenty
from std.complex import Complex
from std.fmt import FormatError

def main() -> Result[(), IoError | FormatError]:
    print(f"{Complex(1.0, 2.0) / Complex(0.0, 0.0)} {Complex(0.0, 0.0) / Complex(0.0, 0.0)}"?)?
    print(f"{Complex(-4.0, 0.0).sqrt()} {Complex(-4.0, -0.0).sqrt()}"?)?
    print(f"{Complex(0.0, 0.0).log()} {Complex(f64.INF, f64.NAN).abs()}"?)?
    print(Complex(0.0, 0.0) == Complex(-0.0, 0.0), Complex(f64.NAN, 0.0) == Complex(f64.NAN, 0.0))?
    Ok(())
```
```output
inf+infi NaN+NaNi
0.0+2.0i 0.0-2.0i
-inf+0.0i inf
True False
```

Equality is the language's structural class equality, not a hook: `0.0` equals
`-0.0`, and a NaN part makes values unequal, as for `f64`.

## Formatting

`Complex` has a public `display` method, so
[f-strings](../language/04-expressions/format-strings.md) and
[`std.fmt.to_string`](fmt.md) write `re+imi` or `re-imi`, each part in the
`f64` print spelling: `1.5+2.0i`, `1.0-0.5i`, `NaN+infi`. A negative zero
imaginary part is written `-0.0i`. Builtin `print` does not use `display` and
writes the structural spelling `std.complex.Complex(re=1.5, im=2.0)`. A format
spec applies to the whole text; format the parts directly, as in
`f"{z.re:.3f}{z.im:+.3f}i"`, to control precision.

## Acknowledgements

- API modelled on Python's [`complex`](https://docs.python.org/3/library/stdtypes.html#numeric-types-int-float-complex) and [`cmath`](https://docs.python.org/3/library/cmath.html); `norm` follows C++ [`std::norm`](https://en.cppreference.com/w/cpp/numeric/complex/norm).
- Division uses Robert L. Smith's [Algorithm 116: Complex division](https://doi.org/10.1145/368637.368661) (Communications of the ACM, 1962).
