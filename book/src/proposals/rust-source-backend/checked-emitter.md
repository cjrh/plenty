# Checked emitter and Rust build costs

This follows the [Rust-source proposal](../rust-source-backend.md). The experiment
in `experiments/rust-source-backend/emitter/` generates Rust from the real checked
frontend. It is deliberately smaller than the ownership-rich [hand-lowered
probes](evidence.md): its purpose is to test a real emission boundary and the
cost of compiling the resulting source, not to declare a production backend.

## Implemented experiment

Build the developer driver with `cargo build --locked --release --features
rust-source-spike --example rust_source_spike --bin plenty`. The hidden emission
API is absent from normal builds. It loads a normal Plenty module, runs frontend and
independent operation/ownership checking on the complete input, then selects
explicit export roots and their reachable functions. There is no second parser,
alternative Plenty type system, public backend flag, or shared-IR redesign.

| Area | Admitted slice |
| --- | --- |
| API/storage | Native signed/unsigned integers through 64 bits, bool, unit results |
| Computation | Checked add/sub/mul, floor division/modulo, wrapping add/sub/mul, integer conversions, comparisons |
| Control flow | Structured branches, scalar matches, short circuiting, while, break/continue, returns and direct calls |
| Generics | Concrete scalar specializations already produced by Plenty; no generated Rust generic functions |
| Recursion | Self-tail calls become an explicit loop with evaluated argument temporaries; other recursion is rejected |
| Rust cooperation | Explicit checked stub → Rust companion replacement, called from a native Rust host through an exported Plenty function |
| Diagnostics | Source-located unsupported operations; sidecar line map demonstrated against JSON rustc type errors |

Every accepted value is a Copy scalar. This avoids inventing a storage,
allocation or lifetime model. References, records, enums/Results, dynamic owners,
floats, generators and concurrency are rejected, not silently converted into
Rust types. Input imports still use Plenty's normal resolver; only reachable
admitted functions are emitted. All input bodies are checked before pruning.

The checked `Op` representation is sufficient for this slice: typed slots,
structured control flow, specializations and source sites survive lowering.
Snapshot temporaries preserve evaluation order; scalar branch values become
Rust tuple joins. Already-validated ownership-only markers have no emitted
action for this Copy-only storage. That does **not** establish that the same
approach suffices for references or destruction.

Generated Rust has no dependencies beyond the installed standard library, no
unsafe code, heap marshalling, trait-object dispatch, macros or derives.
Checked failures call an aborting helper, and the generated module requires
`panic=abort`. It does not reproduce Plenty's trap wording/status, stack guards,
runtime caller traces or debugger source information. A companion is an explicit
integration contract, not automatic discovery of arbitrary Rust APIs.

## Measurement method

`run.py` checks the semantic slice and `bench.py` measures it. Every benchmark
variant compiles a real Rust library, compiles/links a Rust host, and runs it;
its exit-status checksum must match the same Plenty workload compiled through Cranelift.
An edited incremental library is checked against a freshly compiled native
program too. The source generator creates 1, 100 or 1,000 scalar functions,
each with twelve bounded multiply/add/modulo steps, plus an aggregate loop.

The Rust library boundary prevents the host's fixed input from being folded
through the whole library. Library compilation and the tiny host's
compilation/linking are separate measurements; the latter is not a pure linker
timer. `--emit=metadata` is another full invocation to bound checking costs,
not an exact phase subtraction. No linker-only or peak-memory claim is made.

Each artifact-cold sample starts a new compiler process without incremental
reuse. OS pages, the installed Rust sysroot and Plenty runtime artifacts are
warm. The native baseline sets `PLENTY_CACHE=off`; it is not Plenty's warm
compiled-function-cache path. Explicit Rust incremental results distinguish
first build, unchanged source and a real first-function body edit.

All Rust variants use edition 2024, aborting panics, no debug information,
`embed-bitcode=no`, the installed default linker, and no cross-crate LTO, nightly options,
third-party compiler cache or external crates. The
[rustc codegen documentation](https://doc.rust-lang.org/rustc/codegen-options/index.html)
describes codegen-unit parallelism, incremental reuse and omitting unneeded
embedded bitcode. The table below is measured evidence on this machine, not
a promise that these choices dominate for every crate.
Rustc's default local ThinLTO across optimized codegen units remains enabled.

## Observed build tradeoffs

Recorded 2026-10-24 on a Ryzen 7 7840HS, Linux x86_64, rustc 1.99.0
(`b940084d7`, LLVM 23.1.1), Plenty base `be665bb0` plus this experiment.
Three samples per cell; compiler processes were restricted to CPUs 0–7
(four physical cores). The small/flat development runs were quiet. During the
large optimized/grouped runs, another worker performed validation on separate
cores 4–7 (CPUs 8–15); shared package thermals/memory remain a limitation.
Treat small differences cautiously. The large shape effects are much larger
than the sample spread. Raw samples are `flat-measurements.json` and
`grouped-measurements.json` in the experiment directory.

| Functions | Native dev compile/link | Rust inline-check library, opt 0 | Rust helper library, opt 0 | Rust host compile/link after helper library |
| --- | ---: | ---: | ---: | ---: |
| 1 | 41 ms | 33 ms | 24 ms | 49 ms |
| 100 | 55 ms | 509 ms | 154 ms | 46 ms |
| 1,000 | 229 ms | 5,300 ms | 1,452 ms | 48 ms |

The separate Plenty check/emission process adds approximately 2/13/125 ms
for these helper cases. These are independently measured stages, not totals
from a single timer. Generating one monomorphic helper per arithmetic
operation/type reduces repeated Rust checking work and emitted source size
(1,000 functions: 6.4 MB inline versus 2.8 MB with helpers). One versus sixteen
codegen units did not materially help the flat development case. A separate Rust
metadata-only invocation took about 1.26 s versus 1.45 s for the helper library,
suggesting that much of this cost precedes native code generation. This is not
an exact phase partition.

Flat optimized output exposes a much larger problem. The experiment also emits
groups of 32 sorted function symbols into Rust modules, and optionally applies
`#[inline(never)]` to source functions while leaving arithmetic helpers eligible
for normal optimization. This is not a Plenty module cache. Body edits preserve
these groups; inserting symbols can renumber them.

| Helper emission, opt 3 | 100 functions: library build | 1,000 functions: library build | 1,000 functions: execution |
| --- | ---: | ---: | ---: |
| Flat, ordinary inlining | 1,081 ms | 81,920 ms | 4.78 ms |
| Groups of 32 | 391 ms | 4,074 ms | 4.89 ms |
| Flat, source functions not inlined | 758 ms | 10,255 ms | 6.16 ms |
| Groups of 32, source functions not inlined | 337 ms | 2,901 ms | 4.60 ms |

Execution includes process startup and 100 aggregate iterations. Native release
took 5.27 ms in the flat snapshot; these tiny absolute times do not establish
a general Rust runtime advantage. Development helper execution took 21.89 ms
versus native development's 10.45 ms. Full binary sizes, including linked runtime
material, are in the snapshots; they are not generated `.text` sizes.

Grouping and inlining control offer a credible way to avoid pathological
optimized build costs without sacrificing this workload's runtime. The exact
LLVM attribution was not profiled; the controlled emission changes establish
the effect, not a claim about one specific optimizer pass. Grouping made the
development build slower here, so there is no single winning configuration.

For the 1,000-function development library, explicit incremental reuse reduced
the flat first/unchanged/body-edit rustc times to 2,148/795/1,092 ms. Grouped
output measured 1,655/656/697 ms. The edit timings are single observations,
with a checksum checked against a fresh native build. Add emission and host
compile/link for an application edit; these are not full rebuild times or Cargo
no-op timings. Rust still reads and checks the generated crate.

### Final end-to-end comparison

`finalist-measurements.json` adds a single timer around checking/emission,
Rust library compilation and host compilation/linking, with three independent
samples. This run had no competing task builds. However, recorded CPU frequencies
were around 544 MHz, much lower than expected; these absolute numbers must not
be combined with the earlier exploratory snapshots. The native baseline was
remeasured in the same run. No power-management settings were changed.

| Functions | Native dev | Rust helper dev, end to end | Native release | Rust grouped/non-inlined release, end to end |
| --- | ---: | ---: | ---: | ---: |
| 1 | 427 ms | 707 ms | 369 ms | 736 ms |
| 100 | 520 ms | 2,125 ms | 533 ms | 3,541 ms |

This confirms the practical direction of the earlier stage measurements:
the small Rust pipeline is usable, but its build cost grows faster than the
native path. Repeat on a normally clocked, quiet machine before setting any
build-time budget. The raw snapshot records per-variant CPU frequencies.

**Recommendation:** continue the Rust-hosted library experiment with compact
monomorphic helpers and an explicit development/optimized emission policy.
Small libraries can have practical build times, and stable function grouping
can prevent very expensive optimization. This evidence does not make rustc a
replacement for Cranelift's fast build path. Native ownership-bearing API
emission remains the next semantic gate; more backend breadth should not be
inferred from the scalar benchmark.

## Validation and limits

The differential suite covers all admitted integer widths, narrowing/widening,
wrapping arithmetic, negative floor/modulo and `MIN % -1`, branch values,
short-circuit skipped traps, loop exits, scalar pattern matching, specialized
generics, unit calls and a 100,001-step self-tail recursion with swapped arguments.
Overflow, zero division/modulo, division overflow and checked casts fail in both
native and generated debug/release builds. Exact signal numbers are not compared.

Rust → generated Plenty → Rust companion execution is checked with native
integer signatures. A wrong Rust companion type produces a real rustc error
mapped back to its Plenty declaration. Invalid Plenty in an unexported body
still fails frontend checking; references, unsupported numeric types and
unsupported recursion are rejected. A default unwinding Rust build is rejected.

These measurements cannot establish owner cleanup, native Rust borrowing,
fallible allocation, collection interoperability or arbitrary crate support.
They qualify a restricted compiler experiment; the broader proposal's adoption
criteria remain open. Scheduling stays in [B20](../../backlog.md).
