# Optional linear algebra engine

`python3 -B scripts/build_native_engine.py linalg --out target/release` builds
the isolated engine. Normal compiler builds do not build this workspace.
See the [public reference](../book/src/design/stdlib/linalg.md) and
[engine contract](../book/src/design/stdlib/linalg-engine.md).

## Dependency audit

The vendored faer 0.24.4 source is from crates.io, upstream commit
`0539947ffb757a739d7e703a7d2fa0c792a909c1` (path `faer`). Only source,
manifests, README and notices are retained; upstream example/test-data files
are not needed to build the dependency. The repository-level MIT LICENSE,
omitted by the crate archive, comes from the same commit's
[official mirror](https://raw.githubusercontent.com/sarah-quinones/faer-rs/0539947ffb757a739d7e703a7d2fa0c792a909c1/LICENSE).

Changes to upstream Rust source:

- `src/linalg/matmul/mod.rs`: set nano-gemm threshold to `usize::MAX` and
  use fixed 32-by-32 stack-packed tiles (at most 48 KiB of tile buffers for
  complex f64). The general native-scalar dispatcher
  never enters infallibly allocating GEMM.
- `src/linalg/matmul/triangular.rs` and `internal/mod.rs`: disable direct
  private-gemm-x86 overrides; retain faer's generic recursive paths.
- `src/lib.rs`: suppress upstream unused imports/helpers from disabled default
  features. Decomposition algorithms are unchanged; multiplication uses a
  different blocking/summation order.

All calls use `Par::Seq`. Rayon, sparse, random and NPY features are disabled.
The wrapper avoids faer owned matrices, high-level decompositions, global
parallelism, and overloaded arithmetic. It provides initialized borrowed
matrices and fallible scratch (`MemBuffer::try_new`, `Vec::try_reserve_exact`).
Errors are scalar codes; no error message allocation or native handle exists.

The native tests use a global allocator with a thread-local denial budget.
Every allocation position is failed in turn, including allocations made inside
dependencies. A hidden infallible allocation aborts that test process. This
caught the triangular GEMM override during development. Matrix multiplication
is checked under a zero-allocation budget above upstream's threshold; LU, QR,
Cholesky, solves, SVD and both eigen paths are checked with bounded budgets.

Upgrades must re-audit these paths and rerun denial tests and residual tests.
The patch deliberately favors recoverable allocation and bounded working
storage over upstream heap-packed GEMM performance. Do not cite upstream faer timings as
timings for this build.

## Reproducible kernel comparison

`python3 plenty-linalg/bench.py ENGINE [UPSTREAM_CONTROL] --cpu CPU` checks an
all-ones product and reports medians/minima of seven samples. Each sample runs
for at least 80 ms; setup/allocation is outside timing. The control uses this
same C wrapper with unmodified faer 0.24.4, identical features and baseline
`-C target-cpu=x86-64` flags. It is a measurement control only: its hidden
allocations do not meet Plenty's failure contract.

One local run on 2026-10-26, pinned to CPU 15, gave these median milliseconds
for square f64 products using Plenty's 16-byte element slots:

| Size | Allocation-safe engine | Upstream control |
| --- | ---: | ---: |
| 32 | 0.00532 | 0.00535 |
| 128 | 0.15380 | 0.09946 |
| 512 | 10.64561 | 5.27053 |

These are kernel comparisons on one machine, not end-to-end Plenty timings or
a portable speed guarantee. Larger products remain slower than upstream;
preserve allocation-denial and residual tests when improving that path.
