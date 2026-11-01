# Measuring module edits

`scripts/bench_module_edits.py` compares complete builds in fresh compiler
processes. It generates an application with user modules, an edited dependency
and a generic function; by default it also imports `std.http.server`, using only
a small part of it. It does not change the historical `compile_bench` template.

```sh
cargo build --locked --release --features profile --bin plenty
taskset -c 0,2,4,6,8,10,12,14 python3 scripts/bench_module_edits.py \
    /path/to/baseline/plenty target/release/plenty \
    --modules 10 --functions 100 --runs 7 --profile --json /tmp/module-edits.json
```

The compiler paths can name separate worktree builds or saved copies. Choose
CPUs available on the measurement machine and pause concurrent builds and tests.
Each repetition uses a fresh cache directory per compiler, alternates compiler
order, and keeps source paths fixed throughout the comparison.

By default, timings include frontend checking, object emission and process
startup, but not linking. The script also compiles and runs each source variant
once per compiler to check observable results. Options include:

- `--release`: measure optimized executable builds, including linking, and
  compare executable bytes. This covers cross-module scalar substitution; the
  CLI does not expose optimized object-only emission.
- `--import-std ''`: run a control without the broad standard import.
- `--profile`: run separate, untimed profiles after the first repetition. A
  `profile` build records timing rows; `profile-alloc` also records allocations.
  Do not mix instrumented reports with ordinary latency samples.

| Case | Work and correctness check |
|---|---|
| `disabled` | Cache disabled, including the whole-program cache. |
| `populate` | First build with an empty cache. The program entry is initially only a marker. |
| `function_warm` | Second build: function cache entries exist; this build stores the program object. |
| `unchanged` | Third build: requires a whole-program hit and the same object bytes as `disabled`. |
| `body` | Change one ordinary function body; requires a program miss and byte identity with a fresh uncached build. |
| `interface` | Add a parameter and update its caller; requires the same invalidation and identity checks. |
| `generic` | Change a generic body without changing its signature; requires invalidation of its instance and the same identity check. |

Each edit starts from the base program with a current cached base object. The
function cache retains earlier compiled variants, as in normal editing. JSON
records every sample, cache statistics, artifact size and SHA-256, compiler
binary hashes, CPU affinity, and GNU `time` peak RSS in KiB. Timings use a
monotonic clock.

## Interpreting results

Native emission omits unreachable standard definitions only after checking;
this is not incremental frontend checking. Applications using most of an
imported library may benefit less. See [native emission](../design/toolchain/02-architecture/emission.md)
and the separate [per-module cache design](../design/toolchain/02-architecture/cache.md#later-phases).

In a [2026 measurement](module-edits-emission-results.md), the emission change
improved the broad-import workload but showed no benefit for a control without
standard imports. These historical results are workload-specific, not a general
performance guarantee.
