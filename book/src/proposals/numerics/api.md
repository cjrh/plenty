# Numerics API sketch

Companion to the [numerics core proposal](../numerics.md). Every signature
here is hypothetical and may change before implementation. Fences are `text`
because nothing on this page compiles today. Shipped calls, such as
`scan_csv`, `values_f64` and `gg.table`, use their current signatures.

## Shared types (`std.numeric`)

```text
enum NumericError:
    Domain            # invalid argument: empty bracket, negative tolerance
    NoBracket         # f(a) and f(b) have the same sign
    NonFinite         # an input or callback value was NaN or infinite
    Unconverged(Report)
    Linalg(la.LinalgError)
    Allocation(AllocError)

class Report:
    x: list[f64]          # last iterate; one element for scalar problems
    fun: f64              # objective value or residual norm at x
    iterations: i64
    evaluations: i64
```

A successful solve returns a result type for its problem, such as `Root`,
`Minimum`, `Fit`, `Integral` or `Solution`. Each has the fields of `Report`
plus fields specific to the problem, such as the error estimate for
`Integral` and the covariance for `Fit`.

Array helpers:

```text
def linspace(start: f64, stop: f64, n: Positive[i64]) -> Result[list[f64], AllocError]
def logspace(start: f64, stop: f64, n: Positive[i64], base: f64 = 10.0) -> Result[list[f64], AllocError]
def cumsum(values: &list[f64]) -> Result[list[f64], AllocError]
def diff(values: &list[f64]) -> Result[list[f64], AllocError]
def dot(a: &list[f64], b: &list[f64]) -> Result[f64, NumericError]
def norm(values: &list[f64]) -> f64              # scaled, no spurious overflow
def fsum(values: &list[f64]) -> f64              # correctly rounded sum
def polyval(coefficients: &list[f64], x: f64) -> f64
def polyfit(x: &list[f64], y: &list[f64], degree: i64) -> Result[list[f64], NumericError]
def roots(coefficients: &list[f64]) -> Result[(list[f64], list[f64]), NumericError]   # real, imaginary parts
```

`roots` uses companion-matrix eigenvalues from `la.eig`, so it needs the linalg
engine; `polyfit` uses `la.lstsq`.

## Scalar problems (milestone 1)

```text
import std.numeric.optimize as opt
import std.numeric.integrate as integ
import std.numeric.differentiate as d
import std.math as math
from std.math import MathError

def brentq[E, F: Callable[[f64], Result[f64, E]]](
    f: &F, a: f64, b: f64, xtol: f64 = 2e-12, rtol: f64 = 8.9e-16,
    max_iterations: i64 = 100,
) -> Result[opt.Root, NumericError | E]

def newton[E, F: Callable[[f64], Result[(f64, f64), E]]](
    f_and_derivative: &F, x0: f64, ...
) -> Result[opt.Root, NumericError | E]

def minimize_scalar[E, F: Callable[[f64], Result[f64, E]]](
    f: &F, a: f64, b: f64, xtol: f64 = 1.48e-8, max_iterations: i64 = 500,
) -> Result[opt.Minimum, NumericError | E]

def quad[E, F: Callable[[f64], Result[f64, E]]](
    f: &F, a: f64, b: f64, atol: f64 = 1.49e-8, rtol: f64 = 1.49e-8,
    max_subintervals: i64 = 50,
) -> Result[integ.Integral, NumericError | E]

def derivative[E, F: Callable[[f64], Result[f64, E]]](
    f: &F, x: f64, order: i64 = 1, ...
) -> Result[d.Derivative, NumericError | E]
```

`quad` accepts infinite limits (`f64.inf`, `-f64.inf`) and maps them to a
finite interval, as QUADPACK's QAGI does. Its result includes an error
estimate and the number of subintervals used.

```text
def main() -> Result[(), AllocError | IoError | NumericError | MathError]:
    # Root of cos(x) = x
    root = opt.brentq(&(def(x: f64) -> Result[f64, MathError]: Ok(math.cos(x)? - x)), 0.0, 1.0)?
    print(root.x[0])?
    # Gaussian integral over the real line
    g = integ.quad(&(def(x: f64) -> Result[f64, MathError]: math.exp(-x * x)), -f64.inf, f64.inf)?
    print(g.value, g.error)?
    Ok(())
```

## Multivariate problems (milestone 2)

```text
def minimize_bfgs[E, F: Callable[[&list[f64], &mut list[f64]], Result[f64, E]]](
    f_and_gradient: &F, x0: &list[f64], gtol: f64 = 1e-5, max_iterations: i64 = 0,
) -> Result[opt.Minimum, NumericError | E]

def minimize_lbfgsb[E, F: Callable[[&list[f64], &mut list[f64]], Result[f64, E]]](
    f_and_gradient: &F, x0: &list[f64], lower: &list[f64], upper: &list[f64], ...
) -> Result[opt.Minimum, NumericError | E]

def minimize_nelder_mead[E, F: Callable[[&list[f64]], Result[f64, E]]](
    f: &F, x0: &list[f64], ...
) -> Result[opt.Minimum, NumericError | E]

def least_squares[E, F: Callable[[&list[f64], &mut list[f64]], Result[(), E]]](
    residuals: &F, x0: &list[f64], m: i64, ...
) -> Result[opt.Fit, NumericError | E]

def curve_fit[E, F: Callable[[f64, &list[f64]], Result[f64, E]]](
    model: &F, x: &list[f64], y: &list[f64], p0: &list[f64], ...
) -> Result[opt.Fit, NumericError | E]

def root[E, F: Callable[[&list[f64], &mut list[f64]], Result[(), E]]](
    f: &F, x0: &list[f64], ...
) -> Result[opt.Root, NumericError | E]
```

The solver owns the gradient and residual buffers and passes them as
`&mut list[f64]`, so an evaluation allocates nothing. If no gradient is
available, `d.gradient(&f, &x, &mut g)` gives a finite-difference one. Wrap
that in a closure to pass it as `f_and_gradient`. Separate gradient-free
variants of every solver are not provided.

`Fit` adds `covariance: la.Matrix`, computed from the Jacobian at the solution
as `curve_fit` does, and `residual_norm`.

Interpolation constructs an object once and evaluates it repeatedly:

```text
import std.numeric.interpolate as ip

spline = ip.cubic_spline(&x, &y, boundary=ip.Boundary.NotAKnot)?
value = spline.at(2.5)?
values = spline.at_all(&grid)?          # list[f64]
slope = spline.derivative(2.5, 1)?
```

## ODEs, spectra and linear programming (milestone 3)

```text
def solve_ivp[E, F: Callable[[f64, &list[f64], &mut list[f64]], Result[(), E]]](
    rhs: &F, t0: f64, t1: f64, y0: &list[f64],
    method: integ.Method = integ.Method.DormandPrince54,
    rtol: f64 = 1e-3, atol: f64 = 1e-6, t_eval: Option[list[f64]] = Nothing,
) -> Result[integ.Solution, NumericError | E]
```

`Method` is an enum, not a string. A stiff method also takes a Jacobian
callback; without one, it uses a finite-difference Jacobian. `Solution` holds
`t: list[f64]` and `y: list[list[f64]]`, one list per component, so each
component can go straight into a plot `Column.Num`.

```text
import std.numeric.fft as fft

spectrum = fft.rfft(&samples)?            # list[Complex]
frequencies = fft.rfftfreq(len(samples), 1.0 / rate)?
back = fft.irfft(&spectrum, len(samples))?
```

FFT outputs are `list[Complex]` from [`std.complex`](../../design/stdlib/complex.md),
a `@copy` class, so a list stores the values inline.

```text
solution = opt.linprog(&c, a_ub=&a, b_ub=&b, lower=&lo, upper=&hi)?
```

`linprog` minimizes `cᵀx` subject to `A_ub x ≤ b_ub`, `A_eq x = b_eq` and
bounds. The constraint matrices are `la.Matrix`. An infeasible or unbounded
problem is an error variant, not a status code.

## An end-to-end fit

This program shows the milestone 2 workflow: load data, fit an exponential
decay, and plot data and fit. `scan_csv`, `values_f64`, `gg.table` and
`Column.Num` exist today. The `std.numeric` calls are proposed.

```text
import std.dataframe as df
import std.math as math
import std.numeric as num
import std.numeric.optimize as opt
import std.plot as gg
from std.math import MathError
from std.plot import Column, PlotError

def decay(t: f64, p: &list[f64]) -> Result[f64, MathError]:
    Ok(p[0] * math.exp(-p[1] * t)? + p[2])

def main() -> Result[(), AllocError | IoError | df.DataFrameError | NumericError | MathError | PlotError]:
    frame = df.scan_csv("decay.csv")?.collect()?
    t = frame.values_f64("t")?
    y = frame.values_f64("signal")?
    fit = opt.curve_fit(&decay, &t, &y, &[1.0, 0.1, 0.0]?)?
    print(fit.x, fit.residual_norm)?

    grid = num.linspace(t[0], t[len(t) - 1], 200)?
    mut curve = list[f64].with_capacity(len(grid))?
    for s in grid.iter():
        curve.append(decay(s, &fit.x)?)?

    points = gg.table([("t", Column.Num(t)), ("signal", Column.Num(y))]?)?
    line = gg.table([("t", Column.Num(grid)), ("signal", Column.Num(curve))]?)?
    chart = gg.ggplot(points, gg.aes("t", "signal"), [
        gg.geom_point(),
        gg.geom_line(data=line, colour="firebrick"),
    ]?)?
    chart.save("decay.svg", width=6.0, height=4.0)?
    Ok(())
```

The data passes from the dataframe to the solver to the plot as `list[f64]`.
The only copies are `values_f64` out of Polars storage and the model's own
evaluation.
