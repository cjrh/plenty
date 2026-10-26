# Optional linear algebra engine

`python3 -B scripts/build_native_engine.py linalg --out target/release` builds
the isolated engine. Normal compiler builds do not build this workspace.
See the [public reference](../book/src/design/stdlib/linalg.md) and
[engine contract](../book/src/design/stdlib/linalg-engine.md).

## Allocation contract

The engine uses the
[caller-workspace branch of cjrh/faer-rs](https://github.com/cjrh/faer-rs/tree/plenty/fallible-gemm-workspace),
based on faer 0.24.4. `Cargo.lock` pins the exact Git revision, including
faer-traits and the patched private-gemm-x86 backend. The fork documents its
[API and provenance](https://github.com/cjrh/faer-rs/blob/plenty/fallible-gemm-workspace/FALLIBLE_GEMM.md).
The [reference's fork explanation](../book/src/design/stdlib/linalg.md#why-plenty-uses-a-faer-fork)
records why this dependency is necessary and the measured performance.

All calls use `Par::Seq`. Rayon, sparse, random and NPY features are disabled.
The wrapper avoids faer owned matrices, high-level decompositions, global
parallelism, and overloaded arithmetic. It supplies borrowed initialized
matrices and fallible scratch (`MemBuffer::try_new`, `Vec::try_reserve_exact`).
Errors are scalar codes; no error-message allocation or native handle crosses
the boundary.

Caller-owned GEMM workspaces preserve optimized packing without initializing an
owning TLS cache. A fixed four-slot pool reuses them; its mutex is held only for
take/return, and returning a buffer allocates nothing. Cold and concurrent cache
misses reserve fallibly. Excess returned buffers are dropped. The baseline
configuration retains at most approximately 16 MiB of packing buffers.

Native tests fail allocation positions in turn, including inside dependencies,
and check cold-process failure, allocation-free workspace reuse, concurrent
buffers, strided/sentinel storage, and independent decomposition residuals.
The fork additionally tests all four native scalar types and scope unwinding.
Upgrades must rerun these tests and the non-AVX fallback checks.

## Reproducible kernel comparison

`python3 plenty-linalg/bench.py ENGINE [CONTROL ...] --cpu CPU` verifies an
all-ones product and reports medians/minima of seven samples, each at least
80 ms. Initial calls warm the libraries' workspaces. Matrix allocation/setup
is outside timing; native validation and workspace acquisition are included.
Build an upstream control from the same C wrapper with unmodified faer 0.24.4,
without the workspace call/pool, and identical baseline `-C target-cpu=x86-64`
flags. The control's hidden allocations do not satisfy Plenty's contract.

For matched instruction counts, use `perf stat -e instructions:u,cycles:u`
with `--sizes 512 --strides 16 --iterations 100 --cpu CPU`. This performs seven
fixed-count samples; counters include the same Python setup/validation and one
warm-up multiplication. Counts and wall timings are separate evidence.

Offline builds require both registry and Git caches. Run
`cargo fetch --locked --manifest-path plenty-linalg/Cargo.toml` online first.
