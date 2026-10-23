# Profiling the compiler

For fresh-process cold, unchanged and edited builds, see
[Measuring module edits](module-edits.md). That workload complements the fixed
`compile_bench` baseline below and validates cached objects against uncached ones.

The `profile` Cargo feature reports, per compiler phase, the call count, time
percentiles, total time and, with `profile-alloc`, the bytes and number of
allocations. It uses [hotpath](https://crates.io/crates/hotpath), pinned to
one version because its report formats change between releases. The feature
is off by default; without it no hotpath code is compiled and the compiler
binary is unchanged.

## Running a profile

The profile is recorded only when `PLENTY_PROFILE` is set, and is printed when
the process exits:

```sh
cargo build --release --features profile-alloc --example compile_bench
PLENTY_CACHE=off PLENTY_PROFILE=1 taskset -c 0,2,4,6,8,10,12,14 \
    target/release/examples/compile_bench 1000 10 --aot
```

The same works for the `plenty` binary, for example
`PLENTY_PROFILE=1 target/release/plenty --emit-object FILE -o OUT`. Build with
`--features profile` for time alone. Building with either feature replaces the
release build of the compiler in `target/`; pass `--target-dir` to keep both.

| Variable | Effect |
|---|---|
| `PLENTY_PROFILE` | Record and print a profile. Unset, the instrumented functions record nothing. |
| `HOTPATH_OUTPUT_PATH` | Write the report to this file instead of stdout. |
| `HOTPATH_OUTPUT_FORMAT` | `table` (default), `json`, or `json-pretty`. |
| `HOTPATH_ALLOC_CUMULATIVE` | `true` counts a function's allocations with those of the functions it calls. |
| `PLENTY_CACHE=off` | Compile every function; otherwise cached functions skip Cranelift. |

The compiler turns hotpath's live-metrics HTTP server off unless
`HOTPATH_METRICS_SERVER_OFF` is set.

To see what the compiler generated rather than how long it took, use
[`plenty --dump`](../design/toolchain/02-architecture/dump.md): it writes each
function's operations, Cranelift IR before and after optimization, and
assembly, with an index sorted by code size.

## Selecting a benchmark workload

For generated-program dispatch rather than compiler phase attribution, see
[dense-match dispatch](#dense-match-dispatch).

The first two `compile_bench` arguments are generated units (not lines) and
repetitions. A unit contains a protocol, class, enum and functions. Choose one
workload per invocation to avoid mixing source sizes and checking with AOT:

```sh
cargo build --locked --release --features profile --example compile_bench
PLENTY_CACHE=off PLENTY_PROFILE=1 HOTPATH_OUTPUT_PATH=check.txt \
    target/release/examples/compile_bench 1000 10 --workload check --no-warmup
PLENTY_CACHE=off PLENTY_PROFILE=1 HOTPATH_OUTPUT_PATH=scale.txt \
    target/release/examples/compile_bench 1000 10 --workload scale --no-warmup
PLENTY_CACHE=off PLENTY_PROFILE=1 HOTPATH_OUTPUT_PATH=aot.txt \
    target/release/examples/compile_bench 1000 10 --workload aot --no-warmup
```

| Selection | Work performed |
|---|---|
| `--workload check` | Check the requested unit count only. |
| `--workload scale` | Check **four times** the requested unit count only; the metadata shows `requested_units` and the actual `units` checked. No ratio is printed without a base check. |
| `--workload aot` | Compile and link the requested unit count only, including its frontend checks. |
| `--workload check-and-scale` (default) | Check both sizes and print the scaling ratio. |
| `--workload all` or legacy `--aot` | Check both sizes and also compile/link the base size. |

Without `--no-warmup`, each selected workload runs once before its timed
repetitions. That warm-up is excluded from benchmark medians but **included in
the profiler report**. With `--no-warmup`, ten repetitions mean ten calls of the
selected workload. Source generation and benchmark setup are still within the
outer `main` measurement. `workload` and `warmup` appear in the printed metadata;
`--help` lists the options. Conflicting selections and invalid counts fail
before profiling starts.

All modes work without profiling. Features alone never start a session;
`PLENTY_PROFILE` is still required. Use `profile-alloc` for allocation reports.
Disable the cache deliberately for cold-build measurements; for warm-cache
comparisons use an isolated `PLENTY_CACHE_DIR`, retain it between runs, and omit
`PLENTY_CACHE=off`. Cache hits skip Cranelift but still build IR and assemble the
object. `compile_bench` never uses [program objects](../design/toolchain/02-architecture/cache.md#program-objects);
pass `--no-program-cache` to `plenty` for the same warm-function measurement,
since an unchanged program otherwise skips the frontend and code generation. These modes do not collect a whole-test-suite profile.

## Reading the report

Each row is one measured function: the coarse phases of the
[compiler pipeline](../design/toolchain/02-architecture/index.md), such as
`modules::parse`, `frontend::lower_function`, `ownership::check`,
`op::check`, `codegen::build_user_function`, `output::compile`,
`codegen::emit_object` and `toolchain::link`.

Additional rows subdivide the larger phases:

| Parent | Subphase rows and boundaries |
|---|---|
| `output::compile` | `output::cranelift_compile`: Cranelift compilation or cache lookup/compilation, including cache bookkeeping; `output::collect_code`: copy machine code and convert relocations after success; `output::clear_context`: clear reusable compiler state, including after a compilation error. These do not expose individual Cranelift passes. |
| `ownership::check` | `ownership::prepare_futures`, `ownership::initialize_state`, `ownership::check_moves` (outer sequence and final release), and `ownership::check_loans`. |
| `ownership::check_loans` | For bodies with loans: `ownership::loan_graph` (CFG, reachability and loan tables), `ownership::loan_liveness` (fixed point), and `ownership::validate_loans` (conflict checks and diagnostics). The no-loan scan remains in the parent. |
| `frontend::lower_function` | `frontend::setup_function` (initial state, parameters and parameter loans; for a foreign function, its whole lowering), `frontend::lower_function_body` (the outer body-lowering call), and `frontend::finish_function` (return inference and final operation-body construction). |
| `frontend::lower` | Also reports `generics::expand_data` and `tail_calls::validate`; both can return early when there is no relevant work. |

Recursive ownership walks and nested expression/block lowering are not annotated
on every visit. A loan-bearing function produces one graph/liveness/validation
measurement per ownership check, regardless of CFG nesting. Function lowering
can recursively specialize or re-lower a function during inference; those
nested function-level measurements remain inclusive.

- Time is inclusive: `frontend::lower` contains every `lower_function` call.
- `% Total` is relative to the whole run. Functions that run on every core,
  such as `ownership::check` (inside `op::check`), `build_user_function` and
  `output::compile`, sum the time of all threads and can exceed 100%.
- Allocation rows are exclusive by default: bytes allocated by the function
  itself and unmeasured functions it calls, not by measured functions below
  it. Adding subphase rows moves allocation attribution from parents to children;
  compare sums with the same instrumentation, or deliberately use cumulative
  allocation mode. Parent time still includes its children: never sum all rows
  as independent costs.
- A function measured per call costs about 40 ns, so only phase-level
  functions are measured. To find hot functions inside a phase, use a
  sampling profiler such as `perf record` on a build without the feature.
- Allocation counting adds work to every allocation: in the measurement below
  it added about 2% to Cranelift compilation. Compare times from a `profile`
  build.
- The compiler executables and `compile_bench` allocate with mimalloc, and
  `profile-alloc` counts allocations on top of it. Integration tests call the
  compiler in-process with the system allocator, so their timings do not show
  the executables' allocator behavior.
- For thread contention, compare system time and voluntary context switches
  (`/usr/bin/time -f '%e %U %S %w %c %M'`) as well as wall time. Those counters
  include child processes: `compile_bench --workload aot` and `--compile` count
  the linker's threads too, while `plenty --emit-object` counts only the
  compiler.
- hotpath's CPU sampling mode (`hotpath-cpu`) is not enabled: it needs samply
  and relaxed kernel settings, and `perf` already samples the compiler without
  a feature.

On 2026-10-18 (AMD Ryzen 7 7840HS, eight physical cores, `PLENTY_CACHE=off`),
`compile_bench -- 1000 10 --aot` gave the same medians with the feature off,
built but not recording, recording time, and recording allocations: the
fastest of three runs was 646, 648, 644 and 662 ms for the AOT build. Noise
from other load was larger than these differences. A clean release build of
the compiler with the feature compiled 27 more crates and took 10 s longer
(91 s against 81 s with `-j 3`). Those measurements predate the finer subphase
annotations; measure overhead again when evaluating short-function costs and
confirm optimizations on a feature-disabled build. Use sampling to identify
Cranelift passes and hot leaf functions rather than adding per-operation rows.

`cargo test --all-features` builds the feature too. The instrumented functions
then record nothing, because no test sets `PLENTY_PROFILE` except
`tests/test_profile.rs`, which checks the report.

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

## Dense-match dispatch

The 2026-10-22 B19 experiment at `9c994af0` measures dense integer and enum
dispatch with 4, 16, 64 and 256 arms against the then-production comparison chains.
A temporary Cranelift `Switch` patch produces `br_table`; the patch is preserved
as the original experiment. The production policy described below addresses
issue #395. Reproduction, samples and generated-fixture methodology
are in [`examples/dense_match/`](https://github.com/cjrh/plenty/blob/main/examples/dense_match/README.md).

On a Ryzen 7 7840HS, x86-64 Linux, Cranelift 0.131.1, pinned to one CPU, median
wall times over seven shuffled rounds for two million dispatches were:

| Arms | Integer uniform: chain / switch | Enum uniform: chain / switch | Integer 90%-last: chain / switch |
|---|---:|---:|---:|
| 4 | 13.1 / 15.7 ms | 23.0 / 26.9 ms | 8.5 / 9.9 ms |
| 16 | 16.7 / 18.4 ms | 27.2 / 29.2 ms | 11.3 / 10.5 ms |
| 64 | 23.1 / 18.7 ms | 33.9 / 30.8 ms | 22.6 / 10.0 ms |
| 256 | 48.1 / 19.4 ms | 84.0 / 31.2 ms | 68.0 / 11.0 ms |

The same-session baseline and prototype runs interleave. Times include process
startup, all-arm correctness checks, the loop and output; enums also include
borrowed list indexing. They are not pure dispatch latency or integer-versus-enum
ABI comparisons. A Python oracle checks every sequence's checksum; integer
fallbacks include the first out-of-range value and `u64.MAX`.

The 256-arm enum 90%-last case fell from 111.4 to 14.6 ms. First-arm-heavy cases
show why distribution matters: switches regressed at 4/16/64 arms for both
integers and enums; at 256 they improved from 11.7 to 9.2 ms and 18.2 to 14.6 ms.
A balanced Plenty integer decision tree was slower than chains for uniform
4/16/64-arm inputs (14.7/22.9/30.2 ms), and slower than the switch at 256
(37.9 ms). Neither balanced trees nor switches should replace every chain.

At 256 arms, isolated dispatch size fell from 5,515 to 3,656 bytes for integers
and 7,763 to 3,688 for enums, including table data/padding. At four arms it grew
from 91 to 128 and 131 to 152 bytes. Optimized CLIF instruction counts at 256
fell from 1,284 to 521 and 1,549 to 534. Cold compile/link medians for the full
uniform fixtures were 69/65 ms (integer chain/switch) and 95/99 ms (enum);
these include validation/setup code and linking, with only three samples in
separate batches. They establish no reliable compile-time gain.

### Production policy

The production lowerer now selects a jump table for at least 256 distinct
contiguous integer/tag values, in any source order (issue #395). Smaller and
sparse sets retain chains. The threshold deliberately leaves the mixed 64-arm
results alone; it is not claimed to be optimal for every CPU or distribution.
Regression checks cover all eight integer widths, negative/offset ranges,
signed extrema, unsigned maxima, distant 64-bit fallbacks, reversed cases,
alternatives, nested payload ordering, owned/borrowed enum payloads and tail
exits. Exhaustive enum matching retains the invalid-tag trap. The
[emission contract](../design/toolchain/02-architecture/emission.md#match-dispatch)
describes index normalization and bounded table storage.

The harness also offers `--kinds work`: each arm computes a different wrapping
multiply/add over a runtime state argument. This avoids drawing the entire
conclusion from constant-return arms, while remaining a synthetic workload.
It does not establish a benefit for any existing application. No source syntax
or tutorial idiom changes follow from this optimization.

A follow-up paired run against the chain compiler at `70aa0a3e` validated the
production policy on the same host and CPU affinity, using two million
dispatches and seven shuffled rounds (672 checked timed executions):

| 256-case workload | Uniform chain / table | 90%-first chain / table | 90%-last chain / table |
|---|---:|---:|---:|
| Integer | 55.6 / 21.0 ms | 12.2 / 10.5 ms | 79.7 / 12.1 ms |
| Enum | 100.1 / 35.1 ms | 19.7 / 15.6 ms | 127.7 / 17.8 ms |
| Runtime multiply/add | 57.6 / 22.1 ms | 13.5 / 10.5 ms | 82.1 / 11.8 ms |

Isolated dispatch bytes, including table data/padding, fell from 5,515 to 3,646
for integers, 7,763 to 3,674 for enums and 7,623 to 5,754 for runtime arithmetic.
All 26 optimized-IR/assembly dump files for the smaller matches and unchanged
tree controls are byte-identical between compilers.
Full samples, compiler/source hashes and dump indexes are preserved in
[`production-results.json`](https://github.com/cjrh/plenty/blob/main/examples/dense_match/production-results.json).
Cold build samples were collected in separate batches with other builds active
during the baseline batch; they establish **no compile-time gain**. The machine
was not reserved or frequency-locked, so small timing differences in unchanged
controls are noise rather than evidence for tuning smaller matches. This run
does not measure allocation or peak memory, nor justify a broader sparse-table
policy.

## Serial pipeline callbacks

`scripts/bench_pipelines.py` compares an inline range/filter/map/fold loop with
fused pipelines using named callbacks, closure literals, and immutable scalar
captures. All variants do the same checked arithmetic: filter even values,
square each accepted value modulo 1,000, and add the results. Run:

```sh
python3 -B scripts/bench_pipelines.py target/debug/plenty --items 100000000 --runs 7
```

The compiler executable can be a debug build: the harness requests optimized
native output with `--release`, disables caches, checks every result, interleaves
executables and reverses their order on alternate rounds. Compilation and cold
checking of 100 separate functions are measured separately. This is a repeatable
measurement tool, not a timing-sensitive unit test.

Issue #399 recorded the original named-callback pipeline at 1.906 times its
inline-loop baseline. A fresh pre-change run on this host measured 0.160906 s
for the inline loop and 0.283777 s for the pipeline (1.764 times, medians of
seven executions over 100 million items). The direct callback calls survived
fusion. Bounded scalar leaf substitution removes those calls, while SSA mirrors
of immutable scalar captures expose constant divisors to native optimization.

Only straight-line scalar leaf bodies with at most 24 meaningful operations
qualify. Capture mirrors follow local assignments and joins; exclusive borrowing
disables them for the affected local. Checked arithmetic keeps the original
callee source location. Debug code and larger or effectful callbacks retain
their existing behavior. Broader callback substitution needs separate evidence.

On the same host, a quiet post-change run over 100 million items (seven
interleaved executions per variant) measured:

| Variant | Native median | Relative to inline loop | Cold check, 100 functions | Single-program compile |
|---|---:|---:|---:|---:|
| Inline loop | 0.156573 s | 1.000 | 0.011199 s | 0.046002 s |
| Named callbacks | 0.155743 s | 0.995 | 0.011768 s | 0.043120 s |
| Closure literals | 0.169209 s | 1.081 | 0.023402 s | 0.042156 s |
| Scalar captures | 0.154756 s | 0.988 | 0.023753 s | 0.044352 s |

All executions returned `24500000000`; all three pipeline forms meet the
proposal's within-10% runtime target for this workload. Closure literals still
cost about 8% here, and checking 100 closure-heavy functions costs roughly twice
the named-function variant. Single-program compilation is one observation per
variant, not evidence of a compilation speedup. The machine was not frequency-
locked or CPU-pinned; small runtime differences around parity are noise. These
results do not promise the same ratios for larger, branching or mutable callbacks.
