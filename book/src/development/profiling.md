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

## Reading the report

Each row is one measured function: the coarse phases of the
[compiler pipeline](../design/toolchain/02-architecture/index.md), such as
`modules::parse`, `frontend::lower_function`, `ownership::check`,
`op::check`, `codegen::build_user_function`, `output::compile` (Cranelift,
with its cache lookups), `codegen::emit_object` and `toolchain::link`.

- Time is inclusive: `frontend::lower` contains every `lower_function` call.
- `% Total` is relative to the whole run. Functions that run on every core,
  such as `ownership::check` (inside `op::check`), `build_user_function` and
  `output::compile`, sum the time of all threads and can exceed 100%.
- Allocation rows are exclusive by default: bytes allocated by the function
  itself and unmeasured functions it calls, not by measured functions below
  it.
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
(91 s against 81 s with `-j 3`).

`cargo test --all-features` builds the feature too. The instrumented functions
then record nothing, because no test sets `PLENTY_PROFILE` except
`tests/test_profile.rs`, which checks the report.
