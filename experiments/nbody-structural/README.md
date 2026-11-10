# n-body: fixed-size state and end-to-end SIMD

At main `c7ca5a95`, the most promising compiler investment is **scalar replacement
of small, fixed-size numerical aggregates, bounded loop expansion, and SIMD
values that remain native through construction, projection and arithmetic**.
It must expose the whole time-step, not just optimize individual array accesses.

This is an executable feasibility study, **not a delivered compiler speedup**.
The production benchmark and compiler remain unchanged. The generated source
and IR make the intended shape explicit. They show what is attainable and where
public Plenty still loses it; they do not prove that an automatic pass can
recognize every eligible source program cheaply.

## Whole-program results

Ryzen 7 7840HS, x86-64 Linux, 2026-11-10; Plenty 0.8.25, Cranelift 0.137.0,
GCC 16.2.1, Clang 22.1.8, Rust 1.99.1. One million time steps, pinned core 0.
Three `perf instructions:u` observations and nine timing rounds, rotating program
order. Counts include the launcher/startup; times include process startup.
[Raw observations](counts.json) include all 25 builds and timing ranges.

| Executed program | Median million instructions | Median ms |
|---|---:|---:|
| Current Plenty, staged dynamic lists | 1,995.711 | 102.20 |
| Plenty, fused pair loop, still dynamic lists | 856.682 | 53.55 |
| Plenty, five-body scalar locals, unrolled pairs | 672.683 | 50.34 |
| Plenty, fixed local vectors, paired magnitudes | 1,383.683 | 220.85 |
| Plenty, fixed local x/y vectors | 2,024.685 | 202.78 |
| Cranelift scalar SSA, fixed state | 645.496 | 50.52 |
| Cranelift x/y vectors, baseline SSE2 | 493.496 | 43.24 |
| Cranelift x/y vectors + paired magnitudes, SSE2 | 488.496 | 41.37 |
| Same IR, AVX enabled | 389.496 | 38.33 |
| Clang fixed locals, vectorizers disabled, baseline | 495.496 | 47.21 |
| Clang fixed locals, vectorized, baseline | 334.496 | 32.36 |
| Clang fixed locals, native target | 227.496 | 30.66 |
| Rust reference, baseline x86-64 | 451.635 | 35.23 |
| Rust reference, native target | 229.635 | 27.76 |

The executable AVX prototype has **5.12x fewer instructions and takes 2.67x less
time** than current Plenty in this session. It is still 1.70x Rust native's
instruction count, but only 1.38x its elapsed time. Its timing range was
37.66–40.91 ms, versus Plenty's 101.46–104.42 ms and Rust native's 27.18–28.34 ms.
Shared-machine timings are evidence for the direction and scale, not a stable
cross-machine performance guarantee.

Instruction savings do not predict timing proportionally. For example,
Clang native's staged C implementation uses 274M instructions but takes 50.54ms;
its fused version uses 235M and takes 30.65ms. Dependency chains, scheduling and
expensive arithmetic matter. That is why the prototype was executed and timed.

## What each experiment changes

`generate.py` produces independent source probes from the retained benchmark:

- **Fused:** calculate one pair's difference, magnitude and velocity updates in
  one pass, then update positions after all pairs. Positions do not change during
  force calculation, and each body's velocity contributions keep their original
  order. This removes the four scratch buffers and their traversals. No compiler
  change is needed for the observed 2.33x instruction reduction / 1.91x time gain.
- **Locals:** load the 35 fields once, explicitly expand the ten pairs, maintain
  30 mutable position/velocity values through the step loop, and write them back
  afterwards. Reusing temporary bindings avoids the old probe's 256-slot failure.
  This represents scalar replacement plus fixed-trip expansion, rather than merely
  substituting `5` for a list length. It needs a five-body precondition.
- **SIMD locals:** build the two-pair magnitudes with `f64x2`, then extract scalar
  magnitudes. This tests a different vectorization axis from packing x/y.
- **SIMD x/y:** retain each body's x/y position and velocity in local `f64x2`
  values, with z scalar. This corresponds closely to the x/y IR experiment.
  Public `to_tuple` calls within the step still require caller-context work,
  canonical enum/tuple storage, and spill/reload across native call boundaries.
  The loop alone has 1,414 instructions excluding callees; the complete step
  executes about 2,024. Constructors/arithmetic intrinsics alone are insufficient.

`ir.py` emits complete C-callable Cranelift functions over a 35-double state:

1. Scalar SSA fields, one fused operation sequence per pair.
2. x/y in `f64x2`; z scalar; native lane extraction and insertion.
3. The same x/y state, additionally packing two independent squared distances
   for five packed square roots/divides instead of ten scalar ones.

All load the state outside the time-step loop and store after it. The raw-pointer
kernels require a valid 35-double region for positive step counts. That is an
explicit diagnostic ABI precondition, **not permission to omit checks in public
Plenty**. Negative/zero steps return without accessing state. The source probes
may still check/load their fixed fields for those step counts.

The optional AVX experiment changes only the harness ISA's `has_avx` flag.
Vectors remain 128-bit, and no FMA, approximate reciprocal, fast-math, custom
assembly algorithm or dependency patch is used. The assembly wrapper only links
Cranelift's emitted bytes. Baseline harness behavior is unchanged. AVX permits
three-operand arithmetic, avoiding many SSE operand copies; it also changes
register allocation. The AVX harness refuses a host without AVX support.

C probes use ordinary 56-byte records, whereas Plenty's benchmark rows have
128-byte strides. C/IR fix `dt=0.01`; Plenty takes `dt` as a parameter. The scalar
SSA comparison loads/stores outside the loop, so compact row layout is not its
per-step advantage. Constants, scheduling and control shape still differ; the
27-instruction scalar IR/source difference is not a pure layout ablation.

## Where the remaining instructions go

[ledger.py](ledger.py) uses the existing loop parser and accounts for every
instruction in the single backedge of each expanded timestep. [ledger.json](ledger.json)
retains loop addresses and categories. Arithmetic with a memory operand counts
as arithmetic; it is not counted a second time as a load.

| Per-step category | Plenty scalar locals | CLIF scalar SSE2 | CLIF vector pairs SSE2 | CLIF vector pairs AVX | Clang scalar | Clang vectorized |
|---|---:|---:|---:|---:|---:|---:|
| Scalar FP arithmetic | 280 | 280 | 110 | 110 | 280 | 26 |
| Packed FP arithmetic | 0 | 0 | 85 | 85 | 0 | 129 |
| Stack moves | 245 | 232 | 162 | 138 | 98 | 53 |
| Register copies | 132 | 119 | 87 | 12 | 97 | 74 |
| Lane rearrangements | 0 | 0 | 40 | 40 | 0 | 44 |
| Other memory moves | 0 | 0 | 0 | 0 | 8 | 5 |
| Zeroing | 10 | 10 | 0 | 0 | 10 | 1 |
| Integer/control | 5 | 4 | 4 | 4 | 2 | 2 |
| **Total** | **672** | **645** | **488** | **389** | **495** | **334** |

There are no bounds checks or helper calls inside these particular scalar/IR
step loops. This does not describe the public SIMD source loops, which do call
helpers. C and CLIF's arithmetic sequences preserve the source FP expression
order; vectorization combines independent lanes.

The 99-instruction SSE2-to-AVX reduction is exactly 75 fewer register copies and
24 fewer stack moves. The 150-instruction gap between scalar CLIF and scalar
Clang is 134 extra stack moves, 22 extra register copies, two extra control
instructions, offset by eight fewer other memory moves. Both execute 280 scalar
FP instructions. This is evidence of a residual backend scheduling/register-use
and operand-selection gap, not an assertion that 134 spills can all be removed
independently. It deserves a backend reproducer; raising ISA features alone is
not a replacement for exposing the right IR.

Clang's vectorized assembly uses packed arithmetic for both spatial updates and
independent magnitudes (`sqrtpd`, `divpd`). The no-vectorizer ablation rises from
334 to 495 instructions. Rust's fixed-size/unrolled/vectorized shape is described
in the [original comparison](../nbody-codegen/README.md). These matched C source
ablations strengthen the evidence that exposed state and vectorization both
matter; the transformations are interacting, not additive percentages.

## Implication for Plenty's design

The largest architectural opportunity is to keep a small nonescaping numeric
aggregate as a bundle of SSA fields, expanding only small constant-trip loops
and materializing storage only when an address actually escapes. Preserve
floating-point order, ownership, aliasing, traps and cleanup. Use explicit
fixed-size types or proven/guarded lengths; do not globally special-case n-body
or assume every dynamic list has five rows.

The accepted [inline-array design](../../book/src/proposals/value-generics.md)
provides useful syntax for exposing fixed size. **Array syntax or compact
allocation alone is not enough.** Current aggregate lowering in
`src/codegen/inline.rs` and `enums.rs` represents many records by storage addresses;
`portable_simd.rs` has a narrower vector-SSA exception. That exception needs to
survive lane projections, small helper calls/returns, and intermediate tuples.
The public vector probe is an acceptance case for that property. Raising global
inlining limits alone does not establish it and risks unrelated compile-time
and code-size costs.

The fixed scalar probe is an acceptance target for scalar replacement and bounded
expansion; the vector IR is an acceptance target for subsequent SIMD lowering.
Automatic packing of adjacent independent scalar operations (SLP) could recover
Clang's additional gains, but explicit public SIMD should first reach the native
IR shape reliably. A cheap deterministic size/operation budget is necessary;
this study does not measure the compilation cost of a not-yet-implemented pass.

CPU targeting is separable. An **explicit opt-in** target can retain the current
baseline/reproducibility default and include feature choices in caches and
artifacts. The AVX experiment is evidence for such a design, not authorization
to make output silently depend on the build host. Wider-vector selection and
runtime dispatch are not tested here. The measured source fusion is an immediate
source-level option, but the reusable compiler investment is aggregate promotion
and native SIMD continuity. Remaining scope/priorities live in
[B19/B23/B83](../../book/src/backlog.md), with issues #930 and #929.

## Reproduce and validation

From this worktree, with the pinned dependencies and an AVX-capable x86-64 host:

```sh
RUSTC_WRAPPER= cargo build --offline --locked --release --bin plenty
RUSTC_WRAPPER= cargo build --offline --locked --release \
  --manifest-path experiments/kernels/harness/Cargo.toml --target-dir target
python3 -B experiments/nbody-structural/measure.py
python3 -B experiments/nbody-structural/ledger.py target/nbody-structural
```

`--build-only` runs all correctness checks without hardware counters/timings.
Generated sources, CLIF, raw bytes, assembly, executables and counter logs stay in
`target/nbody-structural/`. `generate.py` and `ir.py` can be run separately with
an output directory. The production source is never rewritten.

The 223 program/step cases cover -1, 0, 1, 2, 3, 10, 1,000, 10,000 and 1,000,000
steps. For Plenty, C and CLIF, all 35 fields round-trip through 17-digit output
and compare bit-identically, together with the energy lines. Rust's initial
momentum expression order differs, so its two energy lines use the existing
1e-8 tolerance; negative steps are excluded because Rust's parser differs.
[Validation summary](verification.json) records coverage. Additionally,
`verify.c` checks 256 randomized finite initial states at -1 through 31 steps
for each of six CLIF and four C-local builds: **2,560 exact `memcmp` checks**.

Harness formatting and clippy passed; its test command passed (the standalone
crate currently has no unit tests). Executable differential checks above exercise
the new export path; a negative probe confirmed external-relocation export is
rejected before writing bytes. Source-link checks passed (782 Markdown pages),
367 workflow Python tests passed with four skips, and the complete pinned-mdBook
site passed (1,588 default and 794 development pages). The
compiler/runtime implementation is unchanged, so this investigation does not
claim a new full compiler-suite run or compiler compile-time improvement.
No general optimizer, new public array/SIMD API, target flag, or production
benchmark speedup is delivered by these probes.

## Acknowledgements

- The generated programs and C kernels adapt the repository's n-body port and
  Ilia Schelokov's Benchmarks Game reference. The reference is retained under
  the [Benchmarks Game BSD-3-Clause license](../../benchmarks/game/LICENSE-benchmarks-game.txt).
- The existing [Cranelift harness and loop parser](../kernels/README.md) provide
  ISA-matched lowering and instruction counting. No vendored backend is changed.
