# Using Rust crates: three bounded prototypes

This isolated Cargo project compares real Cranelift callers of generated Rust-ABI
and C-ABI adapters, and a tiny checked Plenty-to-Rust source emitter. It does not
modify the Plenty compiler or introduce a supported dependency format. See the
[proposal](../../book/src/proposals/rust-crate-dependencies.md) and
[recorded observations](RESULTS.md).

## Reproduce

Run from this directory on x86_64 Linux GNU. The executable experiments require
the exact `rustc 1.99.0 (b940084d7 2026-09-28)` build, Cargo, a native linker and
Python 3. The offline commands require the Cargo.lock dependencies already cached;
use `cargo fetch --locked` explicitly if setting up another machine. The script
rejects a different rustc identity/host instead of silently claiming Rust ABI
compatibility. `rustup run 1.99.0 python3 run.py` selects that installed toolchain
if it is not the default.

```sh
rtk proxy python3 run.py
rtk proxy cargo test --locked --offline
rtk proxy cargo clippy --locked --offline --all-targets -- -D warnings
rtk proxy cargo fmt --all -- --check
rtk proxy python3 -B -m unittest test_generator.py
```

The script uses its own `target/`, emits all intermediate sources/object/rlibs,
executes six correctness variants and writes `target/probe-results/results.json`.
It includes expected crashing overflow subprocesses. `RESULTS.md` and
`observed-results.json` preserve one run; subsequent timings naturally differ.
No crates are installed/fetched automatically by the script.

The **separate** compiler-private observation uses installed
`nightly-2026-10-01`, `rustc-dev` and `llvm-tools`:

```sh
rtk proxy rustup component add --toolchain nightly-2026-10-01 rustc-dev llvm-tools
rtk proxy python3 run_abi.py
rtk proxy rustfmt +nightly-2026-10-01 --edition=2024 --check abi-helper.rs abi-input.rs
```

It prints/saves `FnAbi` pass modes for scalars, `&str` and a 24-byte record.
`observed-abi.json` preserves the observation. This helper is not a general
metadata decoder and its output is not consumed by the stable Cranelift path.

## Files and actual execution

| File | Responsibility |
| --- | --- |
| `workload.plenty` | The source used by all three routes, not a fixed expected Rust program. |
| `run.py` | Ten selected API signatures, generated Plenty checker stubs, restricted parser, Rust source/adapters/harness generation and measurement. |
| `src/main.rs` | Calls `plenty::check_source`; emits a real Cranelift object from a JSON operation plan. |
| `src/lib.rs` | Common Rust facade/fixture: scalar, Unicode count, generic identity, real regex crate owner/drop and a local pending-once Future. |
| `abi-helper.rs`, `abi-input.rs`, `run_abi.py` | Optional pinned compiler-private ABI query, distinct from executable tests. |

The actual Plenty frontend checks the source against generated signature stubs,
including class moves and loans. Stubs never execute. A second parser lowers the
checked source: one exactly named `run(value: i64) -> i64` function, simple local
assignments (optionally `mut`) from selected function calls, name/integer/text
arguments, shared/mutable borrow markers, then a sum of local names. Unsupported
control flow, arbitrary expressions and unknown calls are rejected. Python's
AST helps parse that subset; this does not expose or use Plenty's private AST/IR.

Cranelift native imports expand text to pointer/length and owners to pointers.
Both native modes use the same object and differ in generated adapter ABI.
Pointers are represented as I64 only on the explicitly qualified 64-bit target.
The Rust mode matches experimentally observed scalar conventions; it does not
implement arbitrary Rust ABI lowering. Adapter constructors box owners; borrowed
methods reconstruct references, consuming calls reclaim the original Box, and
strings are validated as UTF-8 without payload copies. These unsafe contracts
are trusted fixture inputs, not an interface safe for arbitrary JSON plans.

The Rust-source mode emits statements from the plan and uses native Rust values,
borrows and checked integer addition. The Rust API table is handwritten; the
generator does not discover crates' APIs or verify arbitrary lifetime semantics.
The C mode has generated caller signatures; it is not wired into Plenty's existing
FFI module importer. Both native modes use rustc to link adapter rlibs plus the
Cranelift object, illustrating that C ABI does not require a separate C library
per upstream crate. No path links the actual Plenty runtime into its final binary.

## Validation and limits

All paths check the independent formula `3 * input + 11` for six values, zero
live owners, and rejection of a use-after-move by Plenty. A UTF-8/NUL string has
four scalars/five bytes. Regex produces two matches. The Future is pinned, yields
Pending, then Ready, and gets dropped. Unit tests check explicit poll statuses
and early future destruction. It uses a no-op waker and manual polling, so this
is not executor/wakeup, network-I/O or foreign-callback qualification.

Overflow subprocesses must trap before returning. Rust traps by abort, Cranelift
by an illegal-instruction trap; Plenty's exact diagnostics are not reproduced.
The harness rejects its own fallback-panic marker if arithmetic incorrectly
returns. The source and pointer paths are compared only on this bounded domain.

There is no fallible-constructor/invalid-pattern conversion, OOM injection,
cross-language unwinding, stored borrowed return, shared/mutable concurrent
access, arbitrary Rust trait implementation, aggregates across native calls,
native build-script dependency or cross-target test. Generic identity is a
local fixture; regex is genuine external-crate coverage. The synthetic Future
is Rust's Future protocol, not an async ecosystem crate.

The exact nightly helper demonstrates API access, not a stable compiler API.
A production Rust-ABI route must run its helper and compile adapters with the
same rustc identity, then implement and validate all admitted ABI modes. The
current helper does not consume the stable adapter metadata.

## Measurement interpretation

The first isolated Cargo build reported 85 seconds. This builds the probe tool,
Plenty dependency/runtime packaging and regex in a fresh local target directory;
it is not a clean build of each proposed backend. Recorded adapter/link timings
reuse those debug dependencies. "release" means adapter/generated-source
`opt-level=3`, not a release-built regex fixture or a production Plenty profile.

Whole-workload timings include regex compilation/allocation every iteration.
Scalar timings include loop and call costs, with black-boxed inputs inside each
iteration and black-boxed results. Source mode may inline while native mode has
an object boundary. Three runs are exploratory; no CPU pinning or frequency
control was applied. Do not infer a nanosecond-precise ABI cost or a robust speed
ranking. Binary sizes include debug dependency data and are not stripped.

Code inspection, not allocator instrumentation, identifies two additional Box
allocations per workload for the native adapter handles. All paths allocate the
future's pinned frame and whatever regex needs. Native text parameters borrow
existing static bytes without copying; general Plenty text conversion is untested.
