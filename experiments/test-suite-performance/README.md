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
This measures a narrow source change in this build configuration, not a cold
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

The timed wrapper now supports focused integration filters with the full suite's
feature graph, avoiding an accidental second set of build artifacts. Cargo
remains the default. The optional nextest path retains the repository's test
groups and runs doctests separately. Full-run measurements and remaining
execution hotspots are recorded here after validation.
