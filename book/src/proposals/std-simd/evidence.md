# SIMD spike evidence and backend decision

Evidence for the [portable SIMD proposal](../std-simd.md), collected on
2026-10-23. These experiments do **not** implement a Plenty vectorizer or the
proposed module. They separate current-language semantics, explicit backend
lowering, and another optimizer's recognition of equivalent scalar loops.

This page retains the original 0.131.1 experiment. The implemented clist slice
has separate [Cranelift 0.137 validation](../columnar-list/simd-backend-upgrade.md)
and current measurements; use those for the production backend's present limits.

## Reproduce

The checked-in experiment (`experiments/simd/README.md` in the repository) contains a
locked isolated Rust package, C comparison/verification harness, a Plenty
pipeline program, and a runner that preserves objects, assembly, compiler
remarks, exit status and command timings. No changes to the production compiler
or runtime are required.

Measured environment: AMD Ryzen 7 7840HS, Linux x86-64; Rust `1.99.0`
(`b940084d7`, LLVM 23.1.1); Clang `22.1.8` (Fedora 22.1.8-4.fc44);
Cranelift codegen/frontend/module/object `0.131.1`. The isolated lockfile
resolves some Cranelift helper crates to `0.131.3`; it is separate from Plenty's
root lockfile. Plenty was freshly built from the task worktree based on
`7654ecca`. Default Clang configuration is disabled. Both comparisons use
baseline targets, not `-march=native`; no fast-math or floating contraction.

## Current Plenty chains

`pipelines.plenty` runs these actual fluent chains:

| Input and terminal | Result | What it establishes |
| --- | --- | --- |
| `range(0u32, 1000u32)` → wrapping multiply/add map → wrapping fold | `1505500` | Current syntax and scalar modular oracle for a narrow recognition candidate |
| Same range → even predicate → `count()` | `500` | A filter need not materialize or compact data when the terminal is a count |
| Borrowed `[1e20f32, 1.0f32, -1e20f32, 1.0f32]` → floating left fold | `1.0` | Reassociation changes observable results; a regrouped computation gives `2.0` |

The worktree compiler produced `1505500 500 1.0`, checked by the runner.
This validates scalar semantics and realistic input patterns, not recognition
or a speedup inside Plenty. The C/CLIF kernels below use packed arrays, whereas
these Plenty range pipelines need no array and a general Plenty list does not
promise that representation. Bridging the typed plan to vector IR remains work.

## Native execution and generated code

The Rust probe builds scalar and four-lane CLIF loops for `x * 3 + 7` on `u32`
and `f32`, at `opt_level=speed`. Both use the same loop construction; the
four-lane version is supplied explicitly. The C wrapper runs full blocks then
the scalar remainder. The C comparison gives LLVM ordinary loops over `restrict`
input/output pointers. These pointers are in fact disjoint in the harness.

Native x86-64 checks passed for:

- Integer lengths 0–129, all four-lane remainders, an input offset by one scalar
  from malloc alignment, modular overflow, and an output sentinel after the end.
- Floating lengths 0–9 containing signed zeros, infinities, NaN and minimum
  subnormals. Finite results agree bitwise; NaNs are compared by classification.
- LLVM filter-count results against a scalar count and the ordered floating fold.

These are focused probes, not exhaustive memory-safety tests: a sentinel detects
overwrites, not every possible overread, and sanitizers cannot automatically
instrument the generated CLIF object. Guard-page tests and fuller floating
coverage are adoption requirements. The generated kernel requires a lane-multiple
length; only its wrapper accepts arbitrary lengths.

| Probe | x86-64 | AArch64 |
| --- | --- | --- |
| Scalar CLIF arithmetic map | Executed; scalar integer/`mulss`/`addss` | Object compiled only |
| Four-lane CLIF map | Executed; packed integer operations and `mulps`/`addps` | Object compiled only |
| Eight-lane CLIF map | Rejected: `Unexpected SSA-value type: i32x8` | Same rejection |
| LLVM scalar-source maps | Vectorization width 4; packed arithmetic verified | Assembly shows four-lane arithmetic |
| LLVM scalar-source filter-count | Vectorized; executed and checked | Vectorization remark and emitted assembly only |
| LLVM strict floating fold | Scalar ordered adds; executed | Remark says vectorized, but assembly widens loads and retains ordered scalar adds |

The eight-lane probe stops at its first integer kernel, so it does not independently
test `f32x8` or all target feature settings. AArch64 was **not executed**. No claim
of a production Plenty AArch64 runtime follows from successful object emission.
The floating-fold example shows why an optimizer remark alone is insufficient:
vector loads do not imply a reassociated vector reduction.

## Small runtime comparison

Integer map, nanoseconds per item, best of five trials. Each trial processes
roughly 16 million items through repeated calls; output contributes to a volatile
sink. Allocations and compilation are outside the timed region.

| Items per call | Cranelift scalar | Cranelift four lanes + tail | LLVM from scalar source |
| ---: | ---: | ---: | ---: |
| 7 | 0.533 | 0.544 | 0.448 |
| 1,024 | 0.420 | 0.171 | 0.062 |
| 262,144 | 0.419 | 0.159 | 0.070 |

For the larger sizes, explicit SIMD improves the CLIF loop by roughly 2.5–2.6×;
LLVM is another roughly 2.3–2.8× faster in this kernel. For seven items, vector
setup and the tail erase the benefit. Earlier runs varied by several percent,
with more variation for tiny calls. The CPU was not pinned and frequency was
not controlled; best-of-five suppresses interference but is not a confidence
interval. This is a hot repeated microbenchmark, not a cold-cache bandwidth
test or representative application performance claim.

Assembly helps explain the gap: baseline CLIF expands four-lane multiplication
into `pmuludq` plus shuffles, and reloads constants in the loop. LLVM strength
reduces multiplication by three into adds and interleaves two vectors. This
compares the output of each optimizer on the straightforward equivalent kernel,
not matched instruction sequences. A local strength reduction or stronger
target baseline might narrow the gap; neither was measured here. It would be
premature to infer a general backend ranking from one arithmetic map.

The runner also records process elapsed times: the warm Cranelift object-emission
invocations were around 1–3 ms for two tiny functions, while Clang invocations
were tens of milliseconds for four C functions. These include process startup
and different frontend/object work. They are **not** a controlled compilation
latency comparison. A backend decision needs matched IR and end-to-end Plenty
incremental/cold compilation measurements.

## Source evidence and consequences

Rust checkout `../rust/` was inspected at
`080cc27146389dbb5c8772c476b1937be9c6110f`, under
`library/portable-simd/crates/core_simd/src/`:

- `core_simd_docs.md` describes architecture-independent semantics, scalar
  lowering and older-target subnormal exceptions.
- `vector.rs` defines `Simd<T, N>`, documents wrapping integer operators, and
  implements checked/fallback and masked loads/stores.
- `mod.rs` separates numerical, comparison, pointer and mask facilities. Plenty
  can adopt the useful value model without exposing the entire surface.

Installed Cranelift `0.131.1` source in `src/isa/x64/lower.isle` and
`src/isa/aarch64/lower.isle` contains vector arithmetic lowering, including
`F32X4` and vector multiplication rules. Source inspection motivated the probe;
actual object emission is stronger evidence for these exact tested operations.
It is not evidence for every instruction on every vector type.

Plenty's `src/frontend/pipelines.rs`, `src/codegen/leaf_calls.rs`,
`src/codegen/small_calls.rs` and `src/codegen/scalar_captures.rs` establish the
existing fusion and bounded substitution paths. `src/codegen/collections.rs`
still routes scalar list access through runtime helpers, so callback substitution
alone cannot provide contiguous bulk loads. The current
[number contract](../../design/language/03-types/numbers.md) keeps checked
arithmetic distinct from wrapping methods.

The evidence supports a small portable layer and narrowly recognized pipelines.
It also gives a concrete reason to evaluate LLVM early if broader hardware
utilization becomes a requirement. It does not justify growing an unbounded
Plenty optimizer, promising auto-SIMD for arbitrary chains, or switching
backends before a representative comparison.
