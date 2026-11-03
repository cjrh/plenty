# Numerics crate evaluation

Companion to the [numerics core proposal](../numerics.md). It answers issue
#493, which asked whether `numeris` should back linear algebra, statistics or
optimization, and the native-library half of issue #492, which proposed
optimization through `basin`. Facts are from the crates' docs.rs pages read on
2026-11-03. Neither crate was built or measured.

## Decision

Neither crate is a dependency. `std.linalg` keeps faer. The numerics modules
are written in Plenty and may use either crate as a reference for algorithms
and tests, with attribution.

## The candidates

| | `basin` 3.2.0 | `numeris` 0.6.0 | faer 0.24.4 (shipped) |
| --- | --- | --- | --- |
| Scope | Optimization only: first-order, derivative-free, least squares, global/stochastic, constrained; Brent root finding | Broad and SciPy-like: fixed and dynamic matrices, linalg, ODE, optim, quadrature, interpolation, FFT, special functions, statistics, control, estimation, image processing | Dense linear algebra |
| License | MIT OR Apache-2.0; PRIMA-derived solvers and the L-BFGS-B port are BSD-3 | MIT | MIT |
| First release | 0.0.1 on 2026-04-30 | 0.1.0 on 2026-02-28 | Released since 2021 |
| Churn | About 40 versions; majors 1.0 (June), 2.0 and 3.0 (19–20 October), 3.2 on 2026-11-02 | About 37 versions, pre-1.0, single maintainer | Pinned through a fork |
| Required dependencies | num-traits, rand, rand_chacha, rand_distr, web-time | num-traits | Built only into the separate linalg engine |
| Linear algebra backend | Generic over `Vec<f64>`, nalgebra, ndarray or faer | Its own kernels; optional nalgebra conversion | — |
| Fallible allocation | Not documented | Not documented; dynamic types use `Vec` behind `alloc` | Low-level APIs take caller scratch; Plenty's fork makes GEMM workspaces fallible |

## Reasons

**Callbacks.** An optimizer or ODE solver spends its time calling the user's
function. With a wrapped crate, every evaluation would cross from Rust into
Plenty through the engine's C interface. The callback's environment and its
typed error would have to cross as well, and the error would have to be
carried back out through the crate's own return path, which has no slot for
it. A Plenty solver specializes the callback at compile time and propagates
its error with `?`. The [proposal](../numerics.md#why-plenty-and-not-a-wrapped-crate)
records a probe of this on the current compiler.

**Allocation failure.** The [wrapper rules](../standard-library-scope.md#plenty-or-native)
require a wrapped crate to report allocation failure as `AllocError`.
Neither crate documents fallible allocation, and both allocate through
ordinary `Vec`, which aborts on failure. faer needed a
[fork](../../design/stdlib/linalg.md#why-plenty-uses-a-faer-fork) to meet this
rule for GEMM alone. Repeating that for a second, broader crate would mean
maintaining a second fork.

**Maturity.** Both crates are under a year old. `basin` made three major
releases in a month. `numeris` has one maintainer and is pre-1.0. A
standard library promises stability that its dependencies need to share.
Pinning would work around churn but not supply fixes.

**Replacing faer.** `numeris` reports faster small fixed-size decompositions
than faer on Apple Silicon. That is its stack-allocated `Matrix<T, M, N>`,
which needs const generics. Plenty's matrices are dynamically sized, and
`std.linalg`'s faer integration already passes allocation-denial,
concurrency and non-AVX tests. There is no measured reason to switch.

## What each crate is still useful for

- **`basin`**: the choice and structure of solvers (L-BFGS-B, trust-region
  reflective least squares, the PRIMA derivative-free family) and its test
  problems. Its separation of problem, solver, termination and driver is a
  reasonable model for keeping Plenty's solvers small.
- **`numeris`**: compact reference implementations for adaptive Runge–Kutta
  pairs, RODAS4, Bluestein FFT, natural splines and the special functions in
  milestone 1, with tests that can be checked against SciPy and mpmath.

Ported code keeps its license notice. BSD-3 portions (PRIMA, L-BFGS-B,
SciPy, Cephes) need attribution in the source and the release notices.

## When to revisit

Reopen the choice for a specific algorithm, not a whole crate, if:

- a Plenty implementation is measured to be too slow and the work can be
  done without callbacks into Plenty, as in an FFT kernel writing into a
  caller-allocated buffer; and
- the native code allocates nothing or reports allocation failure, and adds
  its own runtime entry symbol under the
  [wrapper rules](../standard-library-scope.md#plenty-or-native).
