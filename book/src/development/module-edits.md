# Measuring module edits

`scripts/bench_module_edits.py` measures complete object builds in fresh compiler
processes. It keeps the historical `compile_bench` template unchanged. A generated
application imports several user modules and, by default, `std.http.server`;
only a small part of that standard module is needed. Each user module contains
the requested number of independent functions. A separate caller exercises an
edited dependency and its generic function.

```sh
cargo build --locked --release --features profile --bin plenty
taskset -c 0,2,4,6,8,10,12,14 python3 scripts/bench_module_edits.py \
    /path/to/baseline/plenty target/release/plenty \
    --modules 10 --functions 100 --runs 7 --profile --json /tmp/module-edits.json
```

The compiler paths may name separate worktree builds or copies kept before a
change. Every repetition uses a fresh cache directory per compiler; the order of
compiler variants alternates. Source paths stay the same throughout a comparison.
By default the script measures object emission, including frontend checking, and
excludes linking from its timings. It also compiles and runs each source variant
once per compiler to check observable results. `--release` instead measures
optimized **executable** builds, including linking, and compares executable
bytes. This covers cross-module scalar substitution whose dependencies a cache
must respect; the CLI does not expose optimized object-only emission.
`--import-std ''` supplies a control without the broad standard import.
Choose CPUs available on the measurement machine, and run without concurrent
builds or test suites.

| Case | Work and correctness check |
|---|---|
| `disabled` | Cache disabled, including the whole-program cache. |
| `populate` | First build with an empty cache. The program entry is initially only a marker. |
| `function_warm` | Second build: function cache entries exist; this build stores the program object. |
| `unchanged` | Third build: requires a whole-program hit and the same object bytes as `disabled`. |
| `body` | Change one ordinary function body; requires a program miss and byte identity with a fresh uncached build. |
| `interface` | Add a parameter and update its caller; requires the same invalidation and identity checks. |
| `generic` | Change a generic body without changing its signature; requires invalidation of its instance and the same identity check. |

Each edit begins from the base program with a current cached base object. The
function cache retains earlier compiled variants, as it would during normal
editing. JSON records every sample, cache statistics, artifact size and SHA-256,
compiler binary hashes, CPU affinity, and GNU `time` peak RSS in KiB. Timings use
a monotonic clock and include process startup. Profiles requested by `--profile`
are separate, untimed invocations after the first repetition; a `profile` build
records timing rows, and a `profile-alloc` build also records allocations. Reports
from instrumented runs must not be mixed into the ordinary latency samples.

## Scope of the first improvement

Issue #400 removes unreachable embedded-standard definitions from native
emission, after all checking. The pass preserves user definitions, exports,
destructors and generators, and follows explicit calls and callback adapters.
It preserves retained declaration order and cache-on/off object identity. See
[native emission](../design/toolchain/02-architecture/emission.md) for its contract.

Parsing, type resolution, lowering, frame validation and ownership checking still
visit imported standard bodies. The improvement therefore helps native emission
and program-object reuse, but does not make `--check` incremental. Applications
that use most of an imported library should benefit less than a small program
with a broad import. Conservative destructor and generator roots can retain more
functions than execution needs.

Per-module frontend artifacts remain a separate design. Their keys must cover
imported interfaces, body-inferred return facts, generic bodies and native
substitution dependencies. Type graphs, string IDs and generic instance tables
currently belong to the whole program; reusing one module requires explicit
module outputs and deterministic merging. The earlier copy-cost measurements in
the [cache design](../design/toolchain/02-architecture/cache.md#later-phases) still
rule out stored syntax trees as a useful substitute for lowered artifacts.

## Measurements

2026-10-23, AMD Ryzen 7 7840HS, eight physical cores pinned as above. Baseline
`42a4a67c` and the reachability change used release compiler builds with the
`profile` feature inactive during timings. Seven alternating samples per
compiler, with agent builds and tests paused, ten user modules of 100 functions,
and the default broad `std.http.server` import:

| Object build | Before, median ms | After, median ms | Before / after peak RSS, KiB |
|---|---:|---:|---:|
| Cache disabled | 73.93 | 54.73 | 61,228 / 37,360 |
| Empty cache | 77.94 | 54.58 | 62,952 / 38,176 |
| Warm functions | 60.36 | 51.81 | 46,008 / 35,836 |
| Unchanged program | 5.84 | 4.48 | 9,636 / 8,616 |
| One body edit | 60.88 | 50.68 | 45,448 / 35,860 |
| Interface and caller edit | 60.90 | 52.41 | 47,192 / 37,556 |
| Generic body edit | 61.40 | 51.97 | 47,316 / 37,500 |

The base object shrank from 1,368,960 to 408,744 bytes. A separate timing
profile counted 1,558 native user-function builds before and 1,004 after; cold
backend time fell from 30.7 to 7.9 ms. Lowering still took about 21 ms, which
explains why this change does not eliminate the edited-build frontend cost.
The isolated pass took about 0.7 ms, including release of discarded operations.

Separate `profile-alloc` builds of the same workload reported total allocated
bytes of 285.8 → 133.7 MB with caching disabled and 125.1 → 89.2 MB after a body
edit (hotpath's MB units). These are allocation traffic, not peak RSS. Parsing
and body-lowering allocation remained 21.4 and 19.7 MB respectively; most of the
reduction came from avoiding native construction and compilation. Allocation
instrumentation timings are excluded from the latency table.

Repeating the matrix with `--release` (optimized executables, **linking included**)
gave 106.12 → 87.87 ms cold and 95.11 → 82.18 ms after a body edit. Unchanged
linked builds were 36.23 → 35.32 ms: linking dominates this path. The executable
size stayed 3,283,000 bytes because the linker already discarded unused code.
Every measured cached object or executable matched its uncached counterpart;
body, interface and generic-body edits all missed the program cache and produced
the expected changed result.

The same 1,000-function workload with `--import-std ''` produced identical object
sizes before and after. Its cold median was 19.19 → 19.90 ms and body-edit median
18.93 → 19.02 ms; these short runs establish no benefit for that control. The
pass exits after its candidate scan when there are no standard definitions.
These are workload-specific observations, not a general compilation-speed
guarantee or a substitute for measuring an application that uses most of a
library.
