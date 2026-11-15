# Bigint instruction-cost follow-up (issue #987)

This comparison measures direct Plenty changes on top of merged local PR #1278
(`ce4b088f`). It introduces no native kernel, production dependency, threshold
change or public API. The [original experiment](../README.md) defines the
195-case independent Malachite oracle and the 25 API workloads.

## What changed and why

A symbolized Callgrind run of 2,048-bit decimal rendering (10 iterations plus
setup) attributed 12.6% of baseline self instructions to collection reserve,
9.2% to entry pushes, 5.7% to collection addition and 4.6% to growth;
multiplication itself accounted for 9.5%. These are whole-process attribution
percentages, not overhead-subtracted API counters. The retained
[before](profile-before.txt) and [after](profile-after.txt) profiles show why
optimizing the limb multiply alone would leave most work untouched.

- Karatsuba takes borrowed operand slices. Its recursive split no longer copies
  four lists into BigInt values. It adds the three partial products directly
  into a fixed result instead of constructing shifted bigints and adding them
  through the public representation. The existing 32-limb/balancing decisions
  stay the same. Unequal halves, including an empty high half at exactly 2:1,
  are covered by dedicated carry-chain tests.
- Schoolbook multiplication and Algorithm D borrow fixed scratch views. The
  owner cannot resize during those loops, so row addresses remain valid.
  All indexing, arithmetic traps and fallible scratch allocations remain.
- Each conversion shares binary powers and memoizes the exact radix powers
  required by digit-count probes and recursive splits. A borrowed cached power
  avoids copying its limbs. Small parsing keeps its scalar path; single-word
  rendering counts digits by scalar division without constructing powers.
- Immediate integer/bool tuple construction followed by assignment destructuring
  cancels in the frontend, including intervening read-loan markers. Stored or
  borrowed tuples, tuples with managed fields and function-return ABIs retain
  their representation. Primitive low/high and sum/carry values reach codegen
  directly. No check is removed by this transformation.

## Kernels and IR experiment

The existing checked [C limb kernel](../limb.c) still produces the same output
as [Plenty](../limb.plenty). The original attribution is in the parent README.
Before editing the compiler, the dumped optimized CLIF was isolated with only
its diagnostic cold call replaced by the same terminal trap in both variants.
The second variant changes only the tuple slots to scalar values. Both use the
existing harness and Plenty's release flags.

| Build / variant | Static loop union |
| --- | ---: |
| C, gcc 16.2.1 `-O2` | 12 |
| Plenty baseline | 41 |
| Isolated CLIF, tuple slots (`limb-slots.clif`) | 41 |
| Isolated CLIF, scalar fields (`limb-scalars.clif`) | 23 |
| Actual changed compiler (`limb-after-loops.txt`) | 21 |

Tuple slot/padding traffic disappears (17 baseline instructions); allocation
and register layout also change. The actual kernel still duplicates low/high
multiplication, selects a numeric carry through branches, retains its checked
high-word addition and performs extra moves/loop control. The CLIF variant's
23 differs from the real 21 because the isolated cold trap lacks the real
index-diagnostic call and its register-allocation constraints. Static unions
include both carry branches; these are not dynamic per-iteration counts.
C's 12 is still better. The broader baseline schoolbook union was 87; the
whole API results below, rather than that static count, establish the gains.

Reproduce the IR variants:

```sh
RUSTC_WRAPPER= cargo build --locked --release \
  --manifest-path experiments/kernels/harness/Cargo.toml \
  --target-dir target/kernel-harness
for variant in slots scalars; do
  target/kernel-harness/release/plenty-kernel-harness \
    experiments/bigint-comparison/performance/limb-$variant.clif \
    | python3 experiments/kernels/loops.py -v
done
```

## Whole-operation results

The committed [complete table](results/summary.md), [three paired counter
batches](results/instructions.csv) and [five alternating elapsed-time
batches](results/wall.csv) use identical seeded operands and the original
harness. Every checksum matches. Startup/setup/input/output are subtracted
using perf(N, 2N); result creation/destruction and checksums remain inside the
API workloads. The maximum within-shape instruction-counter spread is 0.27%.
Wall times are supplementary host-specific observations.

| Workload | Baseline instructions/op | Changed | Improvement | Malachite |
| --- | ---: | ---: | ---: | ---: |
| 16-bit parse | 3,779 | 1,595 | 2.37x | 153 |
| 16-bit render | 55,132 | 12,788 | 4.31x | 535 |
| 2,048-bit multiply | 503,500 | 180,445 | 2.79x | 10,585 |
| 8,192-bit multiply | 7,464,784 | 1,984,081 | 3.76x | 90,789 |
| 65,536-bit multiply | 244,442,624 | 59,055,892 | 4.14x | 887,101 |
| 65,536-bit floor divmod | 188,952,593 | 180,553,656 | 1.05x | 4,538,185 |
| 65,536-bit parse | 269,119,818 | 85,689,937 | 3.14x | 1,779,608 |
| 65,536-bit render | 1,520,577,301 | 343,264,024 | 4.43x | 4,438,261 |

Baseline medians come from the parent experiment's retained counter CSV;
changed and Malachite medians are from this run on the same Ryzen 7 7840HS,
Linux x86-64 and rustc 1.100.0. Malachite 0.14 remains dev-only.
Addition is essentially unchanged (16-bit 993 to 1,000 instructions). Small
16-bit division regresses from 9,020 to 9,890 instructions (9.6%), and 256-bit
division from 46,468 to 47,563 (2.4%). Callgrind identifies the small-division
change at an inlining boundary: the baseline executes the small path inside
`checked_divmod`, while the changed program calls `quotient_remainder`; active
payload relocation/record traversal dominates both. It is not a change to
floor-division arithmetic. These costs and the still-large Malachite gap remain
within issue #987; this is a bounded improvement, not a claim of competitive
bigint performance or complete resolution of every cost.

[Additional paired counts](results/unbalanced.csv), with shared deterministic
[inputs](results/inputs.json), cover unequal and highly unbalanced multiplication:

| Operand bits | Baseline | Changed | Improvement |
| --- | ---: | ---: | ---: |
| 2,048 x 4,096 | 1,465,330 | 455,300 | 3.22x |
| 4,096 x 2,048 | 1,453,996 | 452,455 | 3.21x |
| 4,160 x 2,048 | 290,516 | 176,282 | 1.65x |
| 4,160 x 2,112 | 1,576,233 | 460,725 | 3.42x |
| 8,192 x 65 | 176,600 | 158,644 | 1.11x |

The balancing boundary explains why the third shape is cheaper than its
neighbours even before this change; no boundary was tuned in this work.

[Compiler instruction counts](results/compiler.csv), three cold-cache CLI
compilations per engine including linking, show limb compilation 121.9M to
121.3M instructions (-0.5%) and the bigint benchmark 624.4M to 658.8M (+5.5%).
The changed std module has more helper bodies. These are compiler/linker
whole-process counts, not `compile_bench aot` or isolated codegen timings.

## Correctness and retained artifacts

[All 16,378 oracle lines match](results/differential-summary.json), including
all radices, negative floor division, exact f64 rounding and Karatsuba boundaries.
The integration suite additionally exercises allocation failures, unchanged
operands/assignment on failure, inline no-allocation operations, traps, unequal
Karatsuba halves and scalar CLIF. No ARM64 execution or throughput, peak memory,
allocation totals or native-kernel crossover is claimed.

The [metadata](results/metadata.json) records host/toolchain/affinity, exact
source and binary hashes, and the baseline archive. Full generated corpora,
outputs, raw perf stdout/stderr, CLIF/asm and executables are retained in the
review worktree's `target/bigint-comparison` and `target/perf987`; the local PR
identifies its durable evidence archive. The binary hashes distinguish these
optimized default programs from test-hook/all-features fixtures.

The original README's reproduction commands apply unchanged using this
worktree's isolated `target/release/plenty`. Supplemental counts run each
`results/inputs.json` pair through baseline and changed bench executables for
32 and 64 multiply iterations, three pairs per engine, with the same
`perf stat -x, -e instructions:u -- taskset -c 0` command. Compiler counts use
`PLENTY_CACHE=off` and the two `--compile FILE -o OUT` sources listed in the CSV.
