# Profiling the compiler

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

## Selecting a benchmark workload

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
object. These modes do not collect a whole-test-suite profile.

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
