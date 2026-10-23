# Three Rust crate prototype results

Companion to [Using Rust crates in Plenty](../rust-crate-dependencies.md).
Reproducible code, commands and checked result snapshots are in
[`experiments/rust-crates`](../../../../experiments/rust-crates/README.md).
These are bounded experiments against Plenty `d7332b6c`, not shipped features.

## One workload, three paths

The original Plenty source executes scalar arithmetic, Unicode/NUL text length,
generic identity, construction/query/drop of a Rust `regex::Regex` owner,
and a local pinned Future polled twice. `regex` 1.13.1 is an actual external
crate resolved by the experiment's Cargo.lock. The future is a synthetic fixture
that returns Pending once, then Ready; there is no network I/O or executor.

The existing Plenty parser/type checker checks the source against generated
signature-only stubs. Those stubs model move-only classes and loans, never run,
and do not automatically prove the selected Rust API matches the declaration.
A second restricted parser converts the same source into an operation plan.
It supports one `run(i64) -> i64` function, simple/mutable local bindings from
selected function calls, name/integer/text arguments, borrow markers and a final
sum of names. It rejects branches, loops, arbitrary expressions and unknown APIs.

The Rust-ABI and C-ABI paths emit real Cranelift machine code from that plan,
calling generated Rust adapters. The generator changes the ABI keyword; both
otherwise use the same scalar/pointer interfaces and Rust-owned handles. Both
link ordinary rlibs through rustc. Cranelift's scalar lowering is experimentally
matched to stable rustc 1.99.0 on x86_64 Linux, not derived from compiler metadata.

The source backend emits Rust statements and checked arithmetic from the plan,
calling the shared Rust fixture directly. Rust owns native values and borrows;
there are no handle-box adapters on that path. This is a real source-lowering
experiment, but it does **not** consume Plenty's internal checked IR, compile
arbitrary Plenty, reproduce its string/allocation model, or execute its runtime.
Passing the public checker before a second parser is a prototype compromise.

## Correctness and failure observations

All three paths passed with adapter/generated-source optimization levels 0 and
3. For inputs -100, -1, 0, 1, 41 and 1000, the independently specified answer is
`3 * input + 11`; live Rust owner counters return to zero. The workload includes
four Unicode scalars in five UTF-8 bytes (`é`, NUL, `a`, `b`) and two regex matches.
The Future's first/second poll yield 0/1; finish requires Ready and a second poll
after Ready is forbidden. Unit tests separately assert these states and dropping
an unfinished Future. This does not qualify foreign callback/cancellation races.

An inserted use-after-move is rejected by the real Plenty checker. Large inputs
trap on integer-sum overflow in all executable paths: Cranelift exits by SIGILL,
generated Rust by SIGABRT. The harness detects an incorrectly successful wrap
followed by its fallback panic, rather than counting any abort as a pass. This
preserves the limited no-wrap contract, not identical Plenty trap diagnostics.

Rust unit tests, Python grammar rejection/literal tests, formatting and Clippy
passed. No compiler/runtime implementation was changed, and no full Plenty
regression suite was run. Native dependency build scripts, real async I/O,
fallible constructor errors, OOM recovery, aggregate ABI calls and cross-target
compatibility remain untested.

## Compiler-derived ABI observation

After installing `rustc-dev` and `llvm-tools`, the separate helper built and ran
on `nightly-2026-10-01` (`21b707e3f`, Rust 1.101.0-nightly). It invokes
`fn_abi_of_instance` after analysis:

| Function input | Observed pass mode |
| --- | --- |
| Rust ABI `i64` | Direct, size/alignment 8 |
| Rust ABI `&str` | Pair, size 16/alignment 8 |
| Rust ABI 24-byte `repr(C)` record | Indirect, Pointer |
| C ABI the same record | Indirect, OnStack |

Both record returns are indirect. This is concrete evidence that the query is
accessible and layout alone does not determine calling convention.
The helper's results are **not fed into the stable Cranelift caller**: compiler
identities differ, and complete pass-mode translation, attributes, monomorphic
API discovery and compatibility checking remain work. Missing initial components
were an environment issue, not evidence the approach was infeasible.
[FnAbi](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_target/callconv/struct.FnAbi.html),
[PassMode](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_target/callconv/enum.PassMode.html).

## Cost observations and interpretation

See the committed [measurement snapshot](../../../../experiments/rust-crates/RESULTS.md)
for exact values and all samples. The initial isolated dependency/tool build took
85 seconds; subsequent fixture builds were warm. Adapter-plus-link builds were
roughly a tenth of a second. Cranelift object emission took a few milliseconds.
These are different stages, not cold-build comparisons of production backends.

Each timing has three runs, with 2,000 whole workloads or two million scalar
iterations, black-boxing inputs **inside** the loop and outputs. Source-mode
Rust may inline; native calls cross an object boundary. Scalar results measure
the whole small loop/call, not a calibrated standalone ABI transition. The
shared fixture and regex dependencies remain debug-built even when adapters are
optimized. CPU contention/frequency, ordering and debug dependencies make these
exploratory measurements, not a backend speed ranking. No benefit of Rust ABI
over C ABI was established.

Both native adapter paths borrow existing UTF-8 bytes without copying payloads
and add two opaque-owner Box allocations per workload. Source mode avoids those
two boxes; all paths retain the future's internal pin allocation and regex's
own allocations. These are source-level accounting observations, not allocator
instrumentation or a general zero-copy guarantee.

Handwritten glue consists of a ten-function API-selection table and the shared
Rust facade/fixture. Unsafe adapter bodies, native caller signatures and checked
source stubs are generated; no handwritten C wrapper or C header is needed.
The Rust-source route calls the same facade directly. Both still rely on a
trusted hand-selected type/ownership contract rather than automatic reflection.

## What changes the recommendation

All three routes can reach the same useful Rust operations. Rust ABI did not
remove conversion or ownership glue, while the helper revealed additional ABI
machinery that must be maintained. C adapters can expose ordinary crates and
future handles using stable compiler interfaces. Rust-source lowering is concise
for this grammar and avoids handle boxes, but a complete backend must preserve
Plenty's fallible allocation, strings, sums, borrowing, cleanup, std and diagnostics.
Therefore generated C adapters are the most tractable next production experiment;
the others remain viable research directions with larger scope.
