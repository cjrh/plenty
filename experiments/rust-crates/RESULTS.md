# Recorded prototype results

Observed 2026-10-23 on x86_64 Linux GNU, stable rustc 1.99.0 (`b940084d7`), LLVM 23.1.1. See [README](README.md) for reproduction, checked scope and measurement limits. Cargo.lock resolves regex 1.13.1 and Cranelift 0.131.3.

All six paths passed six independent numerical checks, zero live-owner checks and overflow rejection. The real Plenty checker rejected use after move. Fixtures use debug-built dependencies; "release" means only adapters/generated source use opt-level 3.

| Path | Adapter + link seconds | Whole workload ns (median) | Scalar ns (median) | Binary bytes |
| --- | ---: | ---: | ---: | ---: |
| Rust-debug | 0.111 | 8009.1 | not measured | 7365752 |
| C-debug | 0.115 | 8462.3 | not measured | 7365752 |
| source-debug | 0.080 | 8012.0 | not measured | 7356944 |
| Rust-release | 0.155 | 8191.5 | 3.90 | 7347392 |
| C-release | 0.173 | 8150.3 | 4.02 | 7347392 |
| source-release | 0.112 | 8320.9 | 2.45 | 7344480 |

Each median uses three executions. Whole workloads repeat 2,000 times; scalar loops repeat two million times. Both black-box input inside each iteration and output. Source code can inline; these are whole-loop/call costs, not isolated ABI overhead. Uncontrolled machine load and debug dependencies prevent a robust performance ranking. [Raw samples](observed-results.json) preserve variance and overflow exit status.

Initial isolated tool/dependency build: 85 seconds (Cargo reported 1m25s). This run's warm Cargo build: 0.059 seconds; Cranelift object emission: 0.0022 seconds. Adapter/link timings exclude dependency compilation and caller emission; executable sizes are unstripped with dependency debug information.

The [separate nightly helper observation](observed-abi.json) uses rustc 1.101.0-nightly (`21b707e3f`, nightly-2026-10-01). It reports Direct for i64, Pair for &str, and Pointer versus OnStack for a 24-byte repr(C) record under Rust versus C function ABI. It is not connected to the stable executable experiments.

Rust ABI and C ABI both require the same generated marshalling/ownership adapters here; code inspection shows two extra Box allocations for their opaque owners, with no string payload copy. Source mode avoids those two boxes, but not the fixture's own allocations. These are structural observations, not measured allocation counts.
