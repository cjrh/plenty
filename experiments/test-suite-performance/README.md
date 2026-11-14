# Test feedback investigation (2026-11-14)

This experiment measures the developer feedback loop, rather than generated
program performance. It preserves the test inventory, assertions, Cargo
profiles, dependency optimization and existing compiled-function/native caches.

## Existing work and current bottlenecks

Local issues #139, #276, #406 and #832 document the earlier consolidation into
one integration binary, fixture-source consolidation, optimized CI compiler,
linker thread limits and runtime/native archive caching. Those optimizations
remain in place. Saved full-suite reports in `target/merged-pr-evidence` show
execution dominating a warm run: PR 1260 spent 251.3 seconds executing and 0.2
compiling. PR 1254's combined run spent 275.7 executing and 85.4 compiling;
its source path and build state differ, so that is not a controlled baseline.

A small edit still rebuilds the integration binary. Although every test module
loads the same helper source, Rust previously instantiated it independently in
hundreds of modules. The baseline binary contains 228 distinct
`support::program` function addresses. This change compiles that exact helper
implementation once at the crate root; compatibility reexports preserve the
existing feature modules. There is one `test_support::program` address after
the change. The helper implementation is byte-identical, has no shared mutable
state, and the exact listing of 6,867 integration tests is unchanged.

## Matched edit probes

Host: AMD Ryzen 7 7840HS, 8 cores / 16 threads, Linux x86-64. Both layouts were
measured in the same dedicated worktree based on `1b60042d`, using Cargo's
development profile, six build jobs, the same full workspace/all-features
integration target and existing artifacts. `RUSTC_WRAPPER=` disabled the user's
global sccache wrapper. The host's clang/wild linker, x86-64-v3 and repository
profile overrides were unchanged. No competing builds ran during these probes.

Each row is the median of three runs. A harmless appended comment triggers a
rebuild; the probe restores the original file afterward. `perf stat` counts
user-space retired instructions for Cargo and its children. Rebuild samples
compile only; they do not include test execution.

| Scenario | Duplicated helpers | Shared helpers | Wall reduction | User instructions before / after |
| --- | ---: | ---: | ---: | ---: |
| Small test edit | 28.24 s | 13.99 s | 50.5% | 169.48 / 111.34 billion (34.3% less) |
| Small frontend edit | 42.48 s | 29.83 s | 29.8% | 300.40 / 242.26 billion (19.4% less) |

The integration binary shrinks from 155,913,744 to 151,044,272 bytes (3.1%).
Both edit scenarios remove about 58.14 billion user instructions; the
frontend scenario still rebuilds the compiler itself. This measures a narrow
source change in this build configuration, not a cold
checkout, production compiler speedup or general guarantee for every edit.

The raw JSON samples are in [results/duplicated-helpers.json](results/duplicated-helpers.json)
and [results/shared-helpers.json](results/shared-helpers.json). Their `warm`
samples show the compiled-function cache warming during the first executions
and are **excluded from improvement claims**. Fully warm selected execution
remains about 0.3 seconds; this change targets rebuild latency.

Reproduce one layout in an isolated worktree:

```sh
python3 -B experiments/test-suite-performance/measure_feedback.py --label shared-helpers --rounds 3
```

To compare layouts, preserve the original root module/helper shim and use the
baseline helper implementation for the earlier layout. Keep worktree path,
feature graph, Cargo configuration and caches fixed; changing worktree source
paths itself invalidates build artifacts. The script saves preparation logs,
per-sample commands, wall times and instruction counts under
`target/test-suite-performance/`. Preparation builds are excluded.

## Runner comparison

The timed wrapper supports focused integration filters with the full suite's
feature graph, avoiding an accidental second set of build artifacts. Build
jobs and test threads are independently selectable. Cargo remains the runner;
the optional nextest wrapper experiment was removed after the following result.

At reviewed implementation `86398e28`, both full suites passed on unchanged
Rust sources. All full-suite artifacts were prepared before either measurement.
Each runner received a separate, identical copy of the same 216-file Plenty
cache (693,116 bytes); neither inherited cache entries from the other run.
Both used six build jobs and sixteen test threads. Order was Cargo then nextest.

| Runner | Compile freshness | Execute including doctests | Total |
| --- | ---: | ---: | ---: |
| Cargo | 0.15 s | 263.30 s | 263.51 s |
| nextest plus separate Cargo doctests | 0.17 s | 261.76 s | 261.98 s |

That 0.6% difference in a single pair is within ordinary timing noise, so there
is **no demonstrated full-suite execution improvement** and no runner change.
Nextest used the existing `ci` nextest configuration with Cargo's `dev` build
profile, preserving the compile-scaling group. It ran the same 7,493 enabled
tests (nine existing ignored tests skipped), followed by the Cargo doctest
phase. Cargo passed 315 compiler units, two CLI units, 6,863 integration tests
(four ignored), nine compile-benchmark tests and 304 runtime units (five ignored).
Both packages currently have zero doctests; the phase remains part of the full
Cargo command. Reports are retained in [results/](results/).

The removed wrapper is reproducible at commit `86398e28`:

```sh
RUSTC_WRAPPER= PLENTY_CACHE_DIR=/absolute/path/cache-cargo just test --build-jobs 6 --test-threads 16
RUSTC_WRAPPER= PLENTY_CACHE_DIR=/absolute/path/cache-nextest just test --runner nextest --build-jobs 6 --test-threads 16
```

Make both cache directories from the same seed before either command. The
ordinary nextest Cargo command remains available as documented in the test
workflow; this experiment adds no dependency on nextest.

## Remaining execution costs

The nextest JUnit report provides a current diagnostic sample, not isolated
per-test CPU profiling. Its test durations sum to 3,897.7 seconds under
16-way contention; the slowest 75 cases (top 1%) contribute 32.8% of that sum.
The [20 slowest cases](results/execution-hotspots.json) include:

| Case family | Observed duration |
| --- | ---: |
| Package relocation with aliases/protocols/conditionals | 133.5 s |
| Package relocation with shadowed names | 64.2 s |
| Generic-instance stress | 62.0 s |
| Package relocation with nested CRLF modules | 55.8 s |
| Workspace-library relocation | 48.3 s |
| Exhaustive eight-bit interval proof | 47.5 s |
| Expanding-generator bound | 36.3 s |
| Imported-module executable-size check | 34.8 s |

The relocation tests intentionally check every candidate move and rerun
programs; generics tests intentionally stress termination limits. Their
coverage is preserved. Large elapsed sums and a nearly unchanged runner pair
point toward reducing repeated work inside expensive cases, rather than
replacing the scheduler. Any such changes need separate coverage-equivalence
and isolated measurements; no test is disabled, sampled or given weaker
assertions here. Existing CI optimization and cache behavior are unchanged.
