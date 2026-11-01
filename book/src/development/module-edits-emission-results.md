# Module-edit emission measurements

These historical measurements assess issue #400's unreachable-standard-definition
emission pass, not current compiler performance. See the [module-edit benchmark](module-edits.md)
for the workload and how to reproduce a comparison, and [native emission](../design/toolchain/02-architecture/emission.md)
for the pass's contract. It runs after checking and retains user definitions,
exports, destructors and generators, following explicit calls and callback
adapters. Conservative roots may retain functions execution does not need.

## Measurements

2026-10-23, AMD Ryzen 7 7840HS, eight physical cores pinned as in the benchmark
example. Baseline `42a4a67c` and the reachability change used release compiler
builds with the `profile` feature inactive during timings. Seven alternating
samples per compiler, with agent builds and tests paused, ten user modules of
100 functions, and the default broad `std.http.server` import:

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
