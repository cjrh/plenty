# Declared-vector bounded prototype (issue #967)

This is the executable evidence stage of approved local proposal PR #1255,
after inline-array prerequisite PR #1262. It does **not** adopt a public
`std.vector`, change production lowering, replace a benchmark, or promise that
an ordinary class containing an array already has native SIMD representation.

The original wide native candidate reduces n-body instructions but does not beat the
existing Plenty program's median cycles on this host. The measured whole-program
gate has therefore **not passed**. A public API or collection layout should not
be adopted from this experiment. Issue #967 remains open for the reviewed design
and evidence decision; this directory delivers its bounded investigation only.

The [paired-magnitude continuation](paired.md) subsequently improves native
whole-program cycles by 6.23–6.61% across eight candidate placements on the
measured AVX2 host. That bounded result adopts no public API or layout.

## Scope and contracts

The recurrence performs three independently ordered `state * scale + delta`
updates. There is no reassociation, FMA, approximate reciprocal/square root or
fast math. Plenty probes cover scalar locals, a direct `array[f64, 3]`, an
ordinary three-field `@copy` domain class, a domain class wrapping that array,
and the existing `std.simd.f64x4` API. The C probe uses the same three components.

`ir.py` constructs three explicitly labelled native SSA variants: three scalar
doubles, two two-lane vectors, and one four-lane vector. The latter two build
private spare lanes as `+0`; a scale's inactive lane is **1 before multiplying**,
including when its logical scale is infinity. Loading/storing always covers
exactly the three logical components, using scalar loads/stores at the boundary.
There is no fourth-word overread, alignment assumption, C vector ABI, heap layout
change or unchecked source-language access justified by this native experiment.
The native C ABI requires valid, nonoverlapping logical state/delta buffers for
positive counts. Counts <= 0 perform no pointer access.

The full kernel uses exactly five bodies, ten pairs in the original order,
scalar ordered `(dx*dx + dy*dy) + dz*dz`, and scalar square root/division.
In particular it does not combine its vector-state substitution with Plenty's
existing adjacent-magnitude square-root/division pairing; that integration is
unmeasured, and these results do not reject every possible native lowering. Its
35-double C state is a diagnostic boundary, not a proposed public storage ABI.
Native SSA state persists across steps. The reused source generator retains
original, fused, local-hoisted, and current SIMD forms; fixed-array variants
change only the container and now-infallible construction. These transformations
are intentionally distinct: native state also removes source ownership/access
machinery, so a difference against the ordinary source form cannot all be
credited to SIMD.

Vec4, general dynamic collection transfer, nested `clist` layout, domain-specific
checked APIs, backend integration and runtime ISA selection are outside this
bounded stage. No public signatures are proposed here, so a `std` signature
conformance report or generated signature-index change is not applicable.

## Reproduce

Run from the repository root, using a release compiler and the pinned harness:

```sh
RUSTC_WRAPPER= cargo build --locked --release --bin plenty
RUSTC_WRAPPER= cargo build --locked --release --manifest-path experiments/kernels/harness/Cargo.toml --target-dir target/kernel-harness
python3 -B experiments/declared-vectors/measure.py --count --compile-count
python3 -B experiments/declared-vectors/native.py --count --cycles
python3 -B experiments/declared-vectors/zero.py
```

The native run executes AVX2 and requires an AVX2/SSE4.1 host; source
`x86-64-v3` binaries require that target too. `perf` needs permission to count
user-mode instructions/cycles. Measurements use physical core 0 by default;
`--core` selects another for the two main runners. Disable competing broad
build/test runs while measuring. No network access or fork changes are needed.
The native script imports the retained n-body generator, IR helpers, drivers and
verifier from `experiments/nbody-structural`; keep that directory available.

Generated source, CLIF, raw function bytes, disassembly, linked binaries and
verification executables go under `target/declared-vectors/`. `measure.py` also
dumps source operations and optimized CLIF for attribution. `results/` retains
the observations below, including all rounds and zero-step controls. Source and
native programs checked all requested step counts (-1, 0, 1, 2, 3, 1000, 1M).
These historical results identify compiler source commit `6bb31653`, merged
unchanged into main by `8d10abd6`; the measurement scripts/harness addition were
uncommitted during collection and are supplied in this experiment's commit.

## Source recurrence attribution

Baseline x86-64 and x86-64-v3, instructions per hot iteration:

| Form | Baseline | v3 | Attribution |
|---|---:|---:|---|
| C | 7 | 7 | two packed FP + two scalar FP + three loop-control |
| Plenty scalar locals | 9 | 9 | six scalar FP + three control; two extra unpaired arithmetic instructions |
| Direct array | 18 | 18 | nine memory moves + six scalar FP + three control |
| Existing SIMD | 16 | 15 | five memory moves; baseline four packed FP, two rearrangements/two copies/three control; v3 two packed FP, three rearrangements/two copies/three control |
| Ordinary domain class | 136 | 136 | repeated aggregate materialization/null-guard paths despite inlining |
| Array-backed domain class | 395 | 395 | more aggregate materialization and slot copies despite inlining |

The scalar arithmetic and array loop have no remaining per-index bounds checks.
The direct array's nine extra memory instructions load/store each component and
reload the three delta components. Helpers in both class loops inline; calls
are not the cause. Static loop-body **unions**, including both arms of guard
diamonds, have 142 instructions for the class (55 memory, 50 stack, 26 control,
5 register copies, 6 scalar FP) and 415 for the wrapper (151 memory, 179 stack,
78 control, 1 copy, 6 scalar FP). Their executed paths have 6 and 20 fewer
instructions respectively. Do not compare these union counts as if all branches
execute on each iteration. The JSON labels them `static_loop_union`.

Zero-step-subtracted process counts agree within 0.014 instruction/step with
these hot-path totals at 1M steps. Residual startup differences include numeric
argument parsing/formatting; subtraction is a control, not exact loop counting.

Median uncached Plenty compilation instructions (three rounds, millions):

| Form | Baseline | v3 |
|---|---:|---:|
| Scalar | 114.541 | 114.753 |
| Ordinary class | 123.504 | 123.759 |
| Direct array | 119.813 | 120.048 |
| Array-backed class | 139.004 | 139.284 |
| Existing SIMD | 171.048 | 171.508 |

These compare different source forms on one compiler. There is no integrated
lowering change and thus no before/after compiler-overhead claim.

## Native substitution

Straight-line recurrence loops have no calls, spills, loads or stores inside
the loop: baseline scalar 9 = six scalar FP + three control; two halves 7 =
four packed FP + three control; AVX2 wide 5 = two packed FP + three control.
All have roughly 6.6M median cycles per 1M iterations. Fewer instructions alone
do not improve this dependent multiply/add chain's latency.

Full native n-body loop attribution, instructions per step:

| Target/form | Total | FP scalar | FP packed | Stack moves | Register copies | Rearrangements/broadcast | Zero | Control |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Baseline scalar | 639 | 280 | 0 | 222 | 123 | 0 | 10 | 4 |
| Baseline halves | 560 | 70 | 140 | 178 | 128 | 30 | 10 | 4 |
| AVX2 scalar | 490 | 280 | 0 | 190 | 6 | 0 | 10 | 4 |
| AVX2 halves | 425 | 70 | 140 | 138 | 33 | 30 | 10 | 4 |
| AVX2 wide | 299 | 70 | 70 | 60 | 5 | 80 | 10 | 4 |

The wide shape saves 70 packed arithmetic and 78 stack moves versus halves,
while adding 50 rearrangement/broadcast instructions and removing 28 copies:
425 - 70 - 78 + 50 - 28 = 299. It still spends 60 instructions on stack
traffic. Scalar sqrt/div and ordered reductions remain, and broadcast/padding
preparation is charged rather than omitted. No fork or lowering was changed to
remove these costs.

## Whole-program gate

Retired instructions/step after zero-step subtraction (three rounds) and median
user cycles per 1M steps (nine rotated, interleaved rounds), Ryzen 7 7840HS:

| Program | Baseline instructions | Baseline cycles M | v3/AVX2 instructions | v3/AVX2 cycles M |
|---|---:|---:|---:|---:|
| Existing Plenty list/original | 473 | 179.36 | 395 | 178.94 |
| Fixed array/original | 2507 | 482.84 | 2482 | 482.82 |
| List/fused | 656 | 254.26 | 485 | 222.40 |
| Array/fused | 1237 | 243.97 | 1152 | 242.01 |
| List/locals | 652 | 222.70 | 468 | 207.36 |
| Array/locals | 633 | 221.67 | 468 | 210.61 |
| List/current SIMD | 516 | 202.35 | 384 | 195.22 |
| Array/current SIMD | 487 | 200.74 | 388 | 196.32 |
| C staged | 734 | 205.73 | 615 | 206.07 |
| C fused | 563 | 221.86 | 422 | 221.44 |
| Native scalar | 639 | 221.86 | 490 | 220.60 |
| Native halves | 560 | 208.38 | 425 | 194.69 |
| Native wide | unavailable | unavailable | 299 | 181.84 |

The wide native form retires ~24% fewer instructions than current v3 Plenty,
but median cycles are ~1.6% higher; sample ranges do not overlap in this session (180.797–182.219M versus
178.274–179.878M cycles). There is no reliable
whole-program speedup. Baseline halves lose ~16% in cycles against existing
Plenty. Copying the current list source directly to a fixed array also regresses
substantially: fixed inline storage alone does not provide register promotion
or cheap borrowed method access. Array/local-hoisted results distinguish this
access/copy overhead from the element arithmetic. Production benchmarks remain
unchanged. Whole-program dynamic counts include callees and are not static
assembly-loop sums.

Cycle samples are observations on a shared host, with outliers retained (notably
C staged v3). `wall_seconds` measures the **entire perf invocation**, including
perf startup, so it must not be used as program wall time. No precise speedup,
portable target policy, or general collection-layout result follows from these
measurements.

## Correctness and safety coverage

All 84 recurrence source output cases match parsed IEEE bit representations.
All 175 n-body/35 recurrence native-driver comparisons match all 35 state
components and both energy values (n-body) or all three components (recurrence).
There are also 10 native-verifier outcomes, for 220 recorded native-run checks.
The five n-body candidates each pass 256 randomized finite initial-state cases
against the scalar C oracle, bit-for-bit. All five recurrence candidates pass
121 combinations of signed zeros, infinities, quiet NaN, smallest subnormals,
minimum normal and maximum finite values, with FE_INVALID parity; NaN payloads
are deliberately not compared. A separate nonzero-logical-state times infinity
case detects invalid operations caused only by private zero padding.

Guard-page probes place each three-double buffer 8 bytes off 16-byte alignment,
ending exactly at an inaccessible page. They exercise both input and output
widths. Null pointers with zero/negative counts prove the early return performs
no access. Native verification relies on the explicit nonaliasing buffer contract;
they do not establish arbitrary source-language borrowing, callback failure,
managed-element cleanup, view lifetime or general C interoperability behavior.
Those remain the responsibility of the compiler's independently tested features.
