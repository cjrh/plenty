# A numerics core for scientific work

**Status: Not implemented.** This page scopes a numerical library for Plenty
and recommends what to build first. It ships no module or API. It replaces
issues #492 (optimization) and #493 (evaluating `numeris`). Scheduling belongs
to [B38](../backlog.md). Shipped neighbours are [`std.linalg`](../design/stdlib/linalg.md),
[`std.stat`](../design/stdlib/stat.md), [`std.math`](../design/stdlib/math.md),
[`std.complex`](../design/stdlib/complex.md), [`std.random`](../design/stdlib/random.md),
[`std.dataframe`](../design/stdlib/dataframe.md) and [`std.plot`](../design/stdlib/plot.md).
Companion pages sketch the [API](numerics/api.md) and record the
[crate evaluation](numerics/crates.md).

## Goal

A graduate student or researcher in a STEM field should be able to load data,
fit a model, solve an equation, integrate a function or an ODE, and plot the
result using only `std`. In Python this needs NumPy and SciPy. Plenty should
cover the parts of them that most research code uses, and stop there. Most of
SciPy is not needed to reach that goal.

## Recommendation

1. **One family, `std.numeric`, in Plenty.** New modules live under
   `std.numeric`: `optimize`, `integrate`, `differentiate`, `interpolate`,
   `special` and `fft`, plus array helpers in `std.numeric` itself. The
   algorithms are written in Plenty. Dense linear algebra inside them calls
   `std.linalg`, which keeps faer.
2. **Adopt neither `basin` nor `numeris` as a dependency.** Use them, with
   SciPy, QUADPACK, MINPACK and Cephes, as algorithm and test-vector
   references. The [crate evaluation](numerics/crates.md) gives the reasons:
   objective functions are Plenty callbacks, neither crate reports allocation
   failure, and both are months old with frequent breaking releases.
3. **Keep the existing modules where they are.** `std.linalg` and `std.stat`
   gain what the new modules need (least squares, more distributions); they
   are not moved under `std.numeric`. A reference index page lists the whole
   numerics core.
4. **Exchange data as `list[f64]` and `la.Matrix`.** These are what
   `std.dataframe` returns from `values_f64`, what `std.plot` takes in
   `Column.Num`, and what `std.linalg` already uses. No new array type.
5. **Deliver in three milestones** ([below](#milestones)), each useful on its
   own, and add further scope only for a stated use case.

## How large is SciPy, and what does Plenty take?

SciPy is a collection of subpackages, each a library in its own right, many
wrapping decades of Fortran and C. Matching it would be a multi-year project
and is not the goal. The table decides each subpackage. **Core** is in the
milestones below. **Later** needs a use case and its own scope before work
starts. **Out** is not planned.

| SciPy / NumPy area | Decision | Plenty scope |
| --- | --- | --- |
| `numpy.ndarray`, broadcasting | Out for now | `list[f64]`, `la.Matrix`, iterators and [`std.simd`](../design/stdlib/simd.md). An N-dimensional array needs indexing overloads, views and a broadcasting design; it is a separate proposal if ever. |
| NumPy helpers | Core | `linspace`, `logspace`, `cumsum`, `diff`, `dot`, `norm`, accurate `fsum`, polynomial `polyval`/`polyfit`/`roots` |
| `numpy.random` | Shipped | [`std.random`](../design/stdlib/random.md); new distributions sample through `std.stat` |
| `scipy.optimize` | Core subset | Scalar roots and minimization; Nelder–Mead, BFGS, L-BFGS-B; nonlinear least squares and `curve_fit`; root finding for systems; dense linear programming |
| | Later | Differential evolution, SLSQP or other constrained NLP, mixed-integer LP, basin hopping |
| `scipy.integrate` | Core subset | Adaptive Gauss–Kronrod `quad`, including infinite ranges; trapezoid/Simpson over samples; `solve_ivp` with Dormand–Prince 5(4) and one stiff method |
| | Later | Events, `dblquad`/`nquad`, boundary-value problems |
| `scipy.differentiate` | Core | Central differences with Richardson extrapolation; gradient, Jacobian and Hessian by finite differences |
| `scipy.interpolate` | Core subset | Linear, cubic spline (natural, clamped, not-a-knot), PCHIP |
| | Later | Smoothing splines, 2-D regular grids, B-spline objects |
| | Out | Scattered-data RBF and triangulation |
| `scipy.special` | Core subset | Gamma, log-gamma, digamma, beta, regularized incomplete gamma and beta with inverses, `erf`/`erfc`/`erfinv` |
| | Later | Bessel functions, elliptic integrals, orthogonal-polynomial evaluation |
| | Out | Hypergeometric and the long tail |
| `scipy.fft` | Core subset | Complex FFT of any length (mixed radix plus Bluestein), `rfft`/`irfft`, `fftfreq` |
| | Later | 2-D FFT, DCT |
| `scipy.linalg` | Shipped, extend | [`std.linalg`](../design/stdlib/linalg.md) plus `lstsq` and `det`, which least squares and fitting need |
| | Later | Matrix functions (`expm`), condition estimates, pseudoinverse |
| `scipy.stats` | Shipped, extend | [`std.stat`](../design/stdlib/stat.md) gains Student's t, χ², F, gamma, beta, binomial and Poisson once `special` exists |
| | Later | Hypothesis tests, linear regression summaries, kernel density estimation |
| `scipy.signal` | Later | Convolution, filtering, peak finding, Welch spectra (`std.signal` already names Unix signals) |
| `scipy.spatial` | Later | k-d tree and distance functions |
| `scipy.sparse`, `sparse.linalg` | Out | A sparse format and iterative solvers need their own proposal |
| `ndimage`, `io`, `odr`, `cluster`, `datasets`, `constants` | Out | [`std.image`](../design/stdlib/image.md) covers images; physical constants may be a small later table |

The core is a few dozen algorithms, each documented in the literature with a
reference implementation and published test problems. That is the size this
proposal commits to.

## Why `std.numeric` rather than top-level modules

`std.linalg` and `std.stat` are top-level, so `std.optimize` would match them.
Top-level names were rejected:

- Several SciPy names are already taken or ambiguous at the top level:
  `std.signal` is Unix signal handling, and `std.special` says nothing about
  numerics.
- One prefix makes the family easy to find in the
  [signature index](../design/stdlib/signatures.md) and keeps shared types
  (tolerances, result records) in one parent module.
- `std` already has this shape: `std.cache.lru` and `std.sketch.xor` sit
  under parent modules.

Moving `std.linalg` and `std.stat` would break shipped programs for no gain;
they stay, and the reference index links them.

## Why Plenty and not a wrapped crate

The [standard library policy](standard-library-scope.md#plenty-or-native)
prefers Plenty unless a native library is clearly better. Here Plenty is
better for three reasons:

- **The user's function is the inner loop.** Root finding, minimization,
  quadrature and ODE solving call a Plenty function thousands of times. A
  callback bound by `F: Callable[...]` is specialized at compile time and can
  be inlined. A native solver would call back across the engine's C boundary
  on every evaluation and would have to carry the callback's error out again.
- **Errors propagate unchanged.** A probe with the release compiler built from `main` on 2026-11-02 compiled
  `bisect[E, F: Callable[[f64], Result[f64, E]]](...) -> Result[f64, RootError | E]`
  with a closure returning `math.MathError`, and a finite-difference gradient
  taking `Callable[[&list[f64]], Result[f64, E]]` and writing into
  `&mut list[f64]`. Both ran correctly. A failing objective, such as `log` of
  a negative number, leaves the solver with its own error, not a sentinel NaN.
- **Allocation failure.** Plenty code reports `AllocError` by construction.
  Neither candidate crate does ([evaluation](numerics/crates.md)).

The dense kernels that justify native code (GEMM, LU, QR, SVD, eigenvalues)
are already native in `std.linalg`. Solvers needing them (Newton steps,
Levenberg–Marquardt, trust regions, interior-point LP) call `std.linalg`.
An FFT written in Plenty may be slower than a SIMD library. If measurement
shows that, the [bigint pattern](bigint.md#recommendation) applies: an
allocation-free native kernel writing into caller-allocated buffers, added
only when it is measured to pay.

## Shared conventions

The [API sketch](numerics/api.md) has signatures. The rules:

- **`f64` only.** As in `std.linalg` and `std.stat`. `f32` and generic float
  code wait until there is demand and a numeric protocol to bound them.
- **Fallible callbacks.** Objectives return `Result[f64, E]`, and solvers
  return `Result[T, NumericError | E]`. One form, not paired fallible and
  infallible variants; an infallible objective writes `Ok(...)`.
- **No allocation per evaluation.** Vector objectives borrow `&list[f64]` and
  write gradients into `&mut list[f64]` and Jacobians into `&mut la.Matrix`
  supplied by the solver. Workspace is allocated once per solve.
- **Non-convergence is an error that keeps the result.** Reaching an
  iteration or evaluation limit returns `Err(NumericError.Unconverged(report))`.
  The report holds the last iterate, function value and counts. SciPy returns
  `success=False` in an ordinary result, which is easy to ignore. In Plenty
  the caller has to handle it, and still gets the last iterate.
- **Options are keyword arguments with documented defaults**:
  `xtol`, `rtol`, `atol`, `max_iterations`, `max_evaluations`. No options
  dictionaries and no method-name strings: each method is its own function,
  so a misspelt method fails to compile.
- **Explicit randomness.** Stochastic methods take `&mut random.Rng` from
  `std.random`; there is no global seed.
- **Nonfinite values.** Inputs are checked; a NaN or infinity from a callback
  is `NumericError.NonFinite`, reported at the evaluation that produced it.
- **Documented accuracy.** Each function's reference page states its method,
  tolerance semantics and measured accuracy over stated input ranges.

## Working with linalg, dataframes and plots

The exchange types already connect the three shipped modules:

| From | To | Today |
| --- | --- | --- |
| `std.dataframe` | numerics | `frame.values_f64("x")` returns `list[f64]` |
| numerics | `std.dataframe` | `df.column_f64("fit", &values)` |
| numerics | `std.plot` | `Column.Num(values)` takes the list without copying |
| numerics | `std.linalg` | `la.vector(&values)` copies; `matrix.to_list()` copies back |

The last row is the weak one. An optimizer converting between a list and a
`Matrix` every iteration would copy every time. Milestone 2 therefore adds
consuming conversions to `std.linalg`, such as `la.vector_from(values)`,
which takes ownership of a list without copying, and `matrix.into_list()`.
Solvers can then keep state in lists and call linalg without copying.

A complete fit, sketched in the [API page](numerics/api.md#an-end-to-end-fit):
read a CSV into a dataframe, take two columns as lists, fit a model with
`curve_fit`, evaluate it on a `linspace` grid, and plot data and fit with
`geom_point` and `geom_line`. Every step uses shipped or core APIs; the
tutorial lesson for milestone 2 is this program.

## Milestones

Each milestone ships reference pages, a tutorial lesson and its tests.

1. **Scalar problems and special functions.** The `std.numeric` helpers;
   `special` (gamma family, incomplete gamma/beta, `erf` family); scalar root
   finding (Brent, Chandrupatla, Newton, secant); bounded scalar minimization
   (Brent); `quad`; `derivative`. `std.stat` gains t, χ², F, gamma, beta,
   binomial and Poisson distributions, which need incomplete gamma and beta.
   This also covers B38's special functions and accurate summation.
2. **Fitting and multivariate problems.** Nelder–Mead, BFGS and L-BFGS-B;
   `least_squares` (Levenberg–Marquardt and trust-region reflective for
   bounds) and `curve_fit`; systems of equations (Powell hybrid);
   finite-difference gradients, Jacobians and Hessians; `interpolate`;
   `la.lstsq`, `la.det` and the consuming conversions.
3. **ODEs, spectra and linear programming.** `solve_ivp` with Dormand–Prince
   5(4) and dense output, plus one stiff method (Radau IIA or a Rosenbrock
   method; chosen by measurement on the Robertson and Van der Pol problems);
   `fft`; dense `linprog` (bounded revised simplex first; an interior-point
   method if needed for larger problems).

After milestone 3, the Later rows above are candidates. Each needs a use case
and its own scope under B38.

## Testing and acceptance

- **Reference values.** Expected values come from SciPy and mpmath, generated
  offline by a checked-in script and stored as fixtures. Tests do not need
  Python.
- **Standard problems.** Moré–Garbow–Hillstrom functions (Rosenbrock, Powell
  singular, Brown badly scaled) for optimization; NIST StRD nonlinear
  regression datasets with certified values for least squares; QUADPACK's
  test integrals, including endpoint singularities, for `quad`; Hairer's
  nonstiff and stiff problems for `solve_ivp`; Netlib LP problems small
  enough to keep in the repository for `linprog`.
- **Accuracy.** Special functions report maximum relative error in ULPs over
  stated ranges. A change that increases it fails the test.
- **Allocation and errors.** Allocation-denial tests cover every solver's
  workspace. Each solver has a test where the callback fails, showing that
  its error reaches the caller.
- **Cost.** Each solver reports evaluation counts on the standard problems.
  Counts are compared with SciPy's. Wall-clock comparisons are optional.
  They follow the [profiling guide](../development/profiling.md) and do not
  promise a speedup.

## Risks and open questions

- **Scope creep.** Requests will arrive for SciPy features not listed here.
  The Later and Out rows are the answer until a use case is written down.
- **Numerical maintenance.** Special functions and stiff ODE solvers are hard
  to get right. Ports keep the source algorithm's structure and credit, so
  upstream fixes can be followed. SciPy and Cephes are BSD-3 and QUADPACK is
  public domain, so ports are allowed with attribution.
- **Fixed-size problems.** Small, fixed-dimension problems (3-vectors,
  quaternions, Kalman filters) would avoid heap allocation with
  [value generics](value-generics.md) (B83). The list-based API does not
  preclude adding fixed-size variants later.
- **FFT speed.** A Plenty FFT may need [`std.simd`](../design/stdlib/simd.md)
  or a native kernel to be competitive. Measure first.
- **Sparse problems.** Large finite-element and network problems need sparse
  matrices, which this proposal leaves out. Researchers with those problems
  will still need another library.
