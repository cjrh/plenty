# Function storage and list-row measurements

Historical optimization measurements; see [Profiling the compiler](profiling.md) for the profiling guide.

## Reusing function storage

The B19 follow-up measured on 2026-10-22 reuses the cleared Cranelift `Function`
already held by each emission worker. Ordinary user functions take its buffers
before building IR and return them through the existing compilation path.
Cache hits and misses both clear function state. Generated adapters and
generator bodies still start with fresh IR; there is no additional pool or
Cranelift patch. Larger buffers can remain until that worker finishes.

On the AMD Ryzen 7 7840HS, pinned to eight physical cores
(`taskset -c 0,2,4,6,8,10,12,14`), a release `profile-alloc` build running
`compile_bench 1000 1 --workload aot --no-warmup` reported these exclusive
allocations (the profiler's KB/MB units):

| Allocation | Before | Reused storage |
|---|---:|---:|
| `build_user_function`, cache disabled, per call | 20.3 KB | 4.9 KB |
| Same phase, all 7,001 calls | 138.5 MB | 33.4 MB |
| Same phase, warm function cache | 138.5 MB | 33.5 MB |
| Whole run, warm function cache | 642.4 MB | 535.5 MB |

Function construction allocated about 76% less; total warm-cache allocation
fell about 17%. These replace the older 34.7 KB/function observation in B19;
other compiler improvements had already reduced that baseline. The whole-program
cache is disabled by `compile_bench`, so a warm run still builds each function's
IR. Baseline and changed 1,000-unit program objects were byte-identical.

Feature-disabled release binaries were then run in three alternating pairs
on the same cores, with no other agent builds or tests running. The default
Plenty compilation mode was used; times include checking, emission, runtime
extraction and linking. Disabled-cache and warm-cache processes used five
timed repetitions after one warm-up; each cold-cache process used a new empty
directory and one repetition without warm-up. Values below are medians of
the three process results; RSS is peak resident memory from `/usr/bin/time`.

| Units | Function cache | Before, ms | Reuse, ms | Before, MiB | Reuse, MiB |
|---:|---|---:|---:|---:|---:|
| 1,000 | Disabled | 438 | 402 | 116.3 | 117.1 |
| 1,000 | Cold | 527 | 534 | 94.9 | 97.1 |
| 1,000 | Warm | 307 | 313 | 120.1 | 120.0 |
| 5,000 | Disabled | 2,317 | 2,261 | 404.1 | 405.8 |
| 5,000 | Cold | 2,263 | 2,385 | 384.6 | 384.4 |
| 5,000 | Warm | 1,620 | 1,799 | 402.5 | 402.6 |

Latency differences changed sign between paired runs. For example, the three
5,000-unit warm-cache pairs changed by +1.6%, +24.2% and -0.4%; these noisy
runs establish no consistent speedup. Peak memory stayed similar on these
many-function workloads. The reason to retain this small change is its
allocation reduction without an extra pool or dependency.

A separate warm-cache `perf stat -e instructions,cycles` comparison pinned to
one core (`taskset -c 0`, 1,000 units, three repetitions plus warm-up) counted
11.811 billion user-space instructions before and 11.532 billion after
(-2.4%); cycles were 9.190 and 9.151 billion. This single pair supports lower
compiler work, while adding no reliable latency claim.

## Inline scalar list rows

Issue #509 found `std.diff`'s Myers loop spending most of its time in runtime
calls: on 3,000 dissimilar lines, `perf` attributed 30% of cycles to
`plenty_list_scalar_get`, 27% to the `Entries::get` it called and 19% to
`plenty_list_scalar_set`, against 21% in the `std.diff` functions. The access
forms were borrowed `&list[i64]` parameters indexed in `while` loops and list
fields of a `&mut` record read and written in the same loop. Only `for` loops
over an unchanged borrowed parameter hoisted a row view, so these forms called
the runtime per element.

Codegen now reads and writes borrowed scalar list rows inline through the
fixed offsets in `plenty-runtime/src/list_layout.rs` ([collections](../design/toolchain/02-architecture/collections.md#narrow-native-helpers)).
Screening came first: `--dump asm` of `std.diff.common_prefix` shows each read
as three loads from the owner, the index normalization, an unsigned compare
and the row load, with a cold call to `plenty_trap_list_index`; afterwards
`perf` attributes 94% of the cycles to `std.diff` functions and none to list
helpers. User-space instructions and cycles (`perf stat`, one run, pinned to
the eight physical cores):

| Benchmark | Instructions before / after | Cycles before / after |
|---|---:|---:|
| `dis.plenty` (3k dissimilar lines, Myers) | 33.31 / 12.85 billion | 7.79 / 3.49 billion |
| `bench.plenty` (all cases) | 81.10 / 38.73 billion | 17.90 / 9.24 billion |
| `large.plenty` (50k lines, 150 edits) | 7.51 / 7.18 billion | 2.59 / 2.37 billion |

Best of three interleaved runs of `bench.plenty`'s five-repetition minimums
(load average 0.5): dissimilar 3k Myers 265 to 100 ms, patience 265 to 100 ms;
repetitive 3k 7.8 to 3.9 ms for both; similar 3k Myers 2.03 to 1.94 ms;
large 50k Myers 38.4 to 36.2 ms, patience unchanged at 54 ms. Edit counts are
identical. The large case is dominated by splitting and interning lines. The
benchmark programs are outside the repository; issue #509 names them.

Compile cost is small: `--compile` of `bench.plenty` rose from 418.9 to 423.0
million instructions (1.0%), its executable's `.text` grew 644 bytes, and
`compile_bench 1000 10 --workload aot` stayed at 76.2 billion instructions.
`std.diff.middle_snake` grew from 3,320 to 4,168 bytes of machine code.
