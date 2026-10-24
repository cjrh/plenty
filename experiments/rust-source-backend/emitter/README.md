# Checked Plenty → Rust scalar emitter

This developer experiment consumes `frontend::compile_file` and `op::check`,
then emits Rust from their checked structured operations. It has no separate
parser or type checker. `examples/rust_source_spike.rs` exposes an unstable
driver; the hidden library module is registered through `src/lib.rs` behind
the opt-in `rust-source-spike` feature. The
normal Plenty CLI and native backend are unchanged.

From the repository root:

```sh
cargo build --locked --release --features rust-source-spike --example rust_source_spike --bin plenty
cargo test --locked --features rust-source-spike --lib rust_source_spike::
python3 -B experiments/rust-source-backend/emitter/run.py
python3 -B experiments/rust-source-backend/emitter/bench.py
```

Both runners accept `--emitter`, `--plenty`, and `--output` absolute paths.
The benchmark also accepts `--sizes`, `--repeats`, and `--iterations`. Use a
quiet machine; report affinity and workload parameters. Outputs stay under
ignored `target/`. The verification runner disables core dumps for deliberate
aborts. Python's standard library, installed rustc and the normal native
Plenty toolchain are sufficient; generated crates have no external dependencies.

`flat-measurements.json` and `grouped-measurements.json` retain the exploratory
snapshots. `--end-to-end` additionally times generation, library compilation and
host compilation/linking under one timer; use `--variants helpers-dev
helpers-release-groups-noinline` to focus on the two finalist configurations.
`finalist-measurements.json` records the fresh end-to-end comparison for 1/100
functions after the user corrected the CPU frequency issue (samples 3.25–4.09 GHz).
It supersedes `finalist-throttled-measurements.json`, retained as provenance for
the earlier roughly 544 MHz run. Neither final run replaces the exploratory
flat/grouped measurements; do not pool different hardware regimes.

For a custom module:

```sh
target/release/examples/rust_source_spike input.plenty generated.rs \
  --export calculate=calculate --helpers
rustc --edition=2024 --crate-type=rlib --crate-name=generated \
  -C panic=abort -C debuginfo=0 -C embed-bitcode=no \
  generated.rs -o libgenerated.rlib
```

Export keys name checked Plenty symbols. Explicit
`--companion host_adjust=crate::host::adjust` replaces a selected checked stub
with a call to that Rust function. The stub is the Plenty-side signature and
reference implementation, not a general Rust API importer. The Rust companion
must be supplied in the crate containing the generated module; Rust checks its
signature. `run.py` demonstrates the full round-trip and intentionally supplies
a wrong companion type to verify source-location mapping.

Admitted storage: integer types through 64 bits and bool, with unit results.
The emitter handles arithmetic checks, Python-style floor/modulo, integer
casts, wrapping add/sub/mul, comparisons, scalar matches, branches, short
circuiting, while loops, break/continue, ordinary calls, and self-tail recursion.
Already-specialized scalar generic functions need no Rust generics. Unsupported
types/operations fail explicitly; this is not a claim of full integer-library
coverage. References, floats, owned values, enums/Results, collections,
generators, foreign C operations and mutual/non-tail recursion are excluded.

`--helpers` shares monomorphic arithmetic helpers; the default repeats the
checked expression at each operation. Both retain Plenty's arithmetic semantics
under Rust optimization. `panic=abort` is explicitly required. Exact Plenty trap
messages/status, caller traces, stack guards, and debug info are not reproduced.
No ownership-bearing storage is admitted, so these tests make no new claim
about cleanup, fallible allocation or borrow translation.

`--module-size 32` groups sorted function symbols into Rust modules. This is an
emission/codegen-unit experiment, not a Plenty module cache. Body edits preserve
groups; inserting symbols may renumber them. `--no-inline` places `inline(never)`
on source functions, retaining ordinary optimizer choices for arithmetic helpers.
The benchmark measures the compile/runtime tradeoff rather than treating this
as a universal optimization policy.

`generated.rs.map.json` maps generated lines to Plenty source locations.
`run.py::mapped_diagnostics` demonstrates mapping JSON rustc diagnostics.
Do not reformat generated Rust without regenerating the map. Compiler errors
are not automatically rewritten by a production CLI.

The benchmark compares inline checks versus helpers, 1 versus 16 codegen units,
optimization levels 0/1/3, and explicit incremental first/unchanged/body-edit
builds. It also tests grouped modules and source-function inlining control;
`--variants` selects cases. The flat 1,000-function optimized variants can take
minutes. Every Rust library is linked into and executed by a real Rust host.
The corresponding native Plenty program must return the same checksum, including
after the body edit. Library compilation and host compilation/linking are timed
separately. `--emit=metadata` is a separate run, not a purported exact frontend
subtraction. Repeated native and Rust process runs include startup costs.

The same generated arithmetic workload feeds both routes. The Rust library
boundary prevents constant propagation from a fixed Rust host into exported
functions. The native baseline uses `PLENTY_CACHE=off`; installed sysroots/runtime
artifacts and OS pages are warm. These are artifact-cold compiler executions,
not machine-cold toolchain builds or Cargo no-op timings. No cross-crate LTO,
linker override, nightly flag or third-party build cache is used. Rustc's default
local ThinLTO across optimized codegen units is left enabled.
