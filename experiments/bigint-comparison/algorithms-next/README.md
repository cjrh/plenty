# Toom-3, packed transforms and recursive output (#987)

This follow-up starts from merged PR #1279 (`43753a18`). It independently
implements further mathematical techniques used by locked Malachite 0.14,
directly in Plenty. No Malachite/FLINT code or tables were copied, and no
production dependency or public API was added. The earlier source provenance,
actual dispatch/profile proof and checked C/Plenty butterfly investigation are
retained in [the preceding report](../algorithm/README.md).

## Algorithm changes

Medium, nearly balanced products use Toom-3: split into three borrowed chunks,
evaluate at 0, 1, -1, 2 and infinity, multiply five point values recursively,
and interpolate five nonnegative coefficients. A magnitude and sign bit handle
the negative evaluation without public BigInt temporaries. If the point
products are `v0`, `v1`, `vm`, `v2`, `v4`, interpolation is:

```text
odd = (v1 - vm) / 2
c2 = v1 - v0 - v4 - odd
c3 = ((v2 - v0 - 16*v4) / 2 - odd - 2*c2) / 3
c1 = odd - c3
```

Divisions are exact; a signed `vm` becomes addition or subtraction of its
magnitude. This follows the same evaluation points as Malachite's active
Toom-3.3 path, independently implementing the published Toom-Cook algorithm.
Current dispatch requires a maximum length of at least 128 limbs, nonempty
high thirds in both inputs, and a ratio at most 5:4. An initial 3:2 eligibility
candidate regressed at 192x129 limbs; its retained measurements justify the
narrower gate. Thresholds from a different implementation are not assumed to
be Plenty crossovers.

The final exact transform uses five independently generated, roughly 60-bit
Proth primes and base-2^128 digits (two u64 limbs each). `constants.py`
regenerates each prime with an exact Proth primality witness and checks the
production call constants against those certificates. No random probable-prime
assumption or third-party prime table is used. Each prime has a 2^40 root
order, well beyond the retained operand-size bound.

Montgomery reduction uses R=2^64 and explicit low/high products. Input loading
reduces a two-word digit and converts it into Montgomery representation;
forward, pointwise and inverse products stay there until the final scale.
Garner mixed-radix conversion reconstructs each coefficient in five scalar
u64 words, adds the previous carry, then emits two words and carries the
remaining three. The conservative coefficient bound 2^20*(2^128-1)^2 is
below the five-prime product. The actual dispatch cap is smaller. The proof
checks 20,000 Montgomery/load cases per prime, 20,000 Garner/carry cases and
20,000 independent Toom interpolation identities.

For the 65,536-bit fixture, transform length falls from the merged baseline's
8,192 to 1,024. Five primes with three full transforms each require 76,800
butterflies instead of 319,488 (4.16 times less). Malachite's source-derived
six-prime, 144-bit-packed, 1,024-slot profile has an untruncated count of
92,160, but uses truncated radix-4 vector transforms, precomputed context and
different scalar arithmetic. Butterfly counts are not equivalent CPU work;
the API instruction/runtime results establish the remaining performance gap.

An intermediate three-prime, 32-bit-digit implementation reduced transform
length to 4,096 and already improved CPU time. Its exact binary, proof and
measurements remain labelled in the archive; it is not a second production
path. The final path retains the minimum size, 2:1 ratio, seven-eighths
occupancy and combined-limb caps, computing occupancy from actual padded
128-bit input-digit counts, including odd limb lengths.

Karatsuba and Toom-3 write low and high point products directly into disjoint
regions of the caller's zeroed output. Interpolation reads those regions
before adding the middle coefficients. This removes two owning recursive
product arrays per node; evaluation/middle scratch remains fallibly allocated.
An error discards the partially written private output while preserving both
borrowed inputs. There is no global scratch/context cache.

## Final measurements

The harness uses optimized default programs, shared deterministic operands,
three paired perf(N,2N) instruction batches and five alternating elapsed-time
batches pinned to CPU 0. Construction, destruction and result checksums remain
inside each operation; fixed process/input/output setup is subtracted from
instruction counts. Exact binaries, raw counters and timings are retained.
The baseline is the exact merged-PR-1279 executable, not a rebuild with current
compiler sources. Malachite remains locked to 0.14.0 with the source provenance
and active dispatch proof in the preceding report.

| Multiplication shape | Merged baseline instructions | Final instructions | Malachite instructions | Baseline CPU ms, median [min,max] | Final CPU ms, median [min,max] |
| --- | ---: | ---: | ---: | --- | --- |
| 2,048 bits | 180,451 | 142,123 | 10,585 | 0.00759 [0.00756,0.00778] | 0.00605 [0.00603,0.00611] |
| 8,192 bits | 1,984,122 | 1,193,785 | 90,790 | 0.08326 [0.08238,0.08569] | 0.05114 [0.05090,0.05173] |
| 65,536 bits | 40,837,351 | 16,305,706 | 887,166 | 3.266 [3.254,3.409] | 1.132 [1.127,1.154] |
| 2,048 × 2,048 limbs (131,072 bits) | 84,355,305 | 33,922,678 | 1,781,980 | 7.080 [6.895,7.235] | 2.394 [2.386,2.709] |
| 1,791 × 1,791 limbs | 108,388,202 | 32,276,195 | 1,608,045 | 7.229 [7.217,7.601] | 2.310 [2.297,2.609] |
| 1,024 × 2,049 limbs (schoolbook) | 66,911,446 | 62,710,813 | 1,589,259 | 11.127 [11.108,11.153] | 9.903 [9.898,9.927] |

Large multiplication improves 2.50x/2.49x in instructions and 2.89x/2.96x in
CPU time at 65,536/131,072 bits. The remaining instruction gaps to Malachite
are **18.38x and 19.04x**, down from approximately 46x and 47x. At 8,192 bits
the gap falls from approximately 21.9x to 13.15x; at 2,048 bits it is 13.43x.
These remain substantial gaps, and issue #987 remains open.

The [full instruction observations](results/instructions.csv) and
[CPU samples](results/cpu.csv) include all 25 original API workloads and 37
threshold, ratio, occupancy and unbalanced multiplication controls. No final
workload regresses by more than 1% in instruction count. CPU results vary:
inline 16-bit add/multiply/render medians are 5.9%/8.4%/5.5% slower, with
sub-microsecond operations and overlapping ranges for add/multiply. There is
no claim that every elapsed-time workload improves. The 65,536-bit parse
fixture falls from 85.69M to 64.09M instructions and 4.891 to 3.916 ms;
rendering falls from 343.36M to 314.21M and 23.541 to 21.353 ms. They still
cost 36.0x and 70.8x Malachite's instructions, respectively.

Earlier candidates remain labelled in the archive: Toom-3 alone, three-prime
32-bit packing, direct recursive output, five-prime packing, and the final
direct schoolbook entry. The direct entry avoids routing obvious basecase
products through the recursive fallible dispatcher; it fixes an intermediate
9.4% instruction regression at 1,024 × 2,049 limbs. The final result above
improves that same control by 6.3%. Every phase uses the same baseline and
deterministic corpus; phase reports are historical evidence, not final results.

## Generated kernel evidence

The extracted production forward transform in [kernel.plenty](kernel.plenty)
and checked [kernel.c](kernel.c) use the same 1,024 data elements, 512 roots,
integer Montgomery operations and repetition checksums. The C version retains
explicit arithmetic and root/block bounds checks and has no `restrict`
assumptions. GCC uses `-O2 -fno-pie -no-pie`; Plenty uses optimized defaults.
Three paired 16/32-repetition batches match checksums and require medians of
**335,653 Plenty instructions versus 219,284 C instructions** per transform
(1.53x). [Raw paired counts](results/kernel.json) retain all observations.

Assembly/optimized CLIF are retained in the evidence archive. The relevant
butterfly loop has a 59-instruction static loop union in Plenty versus 41 in
C; [annotated Plenty loops](results/kernel-plenty-loops.txt) and
[C loops](results/kernel-c-loops.txt) distinguish static instructions from
dynamic counts. Inspection attributes the extra work to duplicated low/high
multiply pairs, branch-selected carry instead of `adc`/`setb`, repeated high
slice bounds checks, root-index normalization, retained arithmetic-overflow
branches, step overflow, and a spilled/reloaded step. No compiler or fork
change is included. This kernel ratio cannot explain the entire public-API
gap; truncated/vector transforms, precomputed context, owning representation
and recursive/helper costs remain distinct opportunities.

## Correctness and qualification

The [constant proof](results/constant-proof.json) certifies every prime,
production transform constant and production CRT inverse literal. The
[product oracle](results/product-oracle.json) compares 608 signed products
against Python exact integers and Malachite, covering Toom thirds/negative
evaluation, the 5:4 seam, transform thresholds/padding/odd lengths, carries
and both unbalanced directions. The [original all-radix oracle](results/all-radix-oracle.json)
also matches 16,378 output lines across 195 cases, including division estimate
corrections. Exact input/product hashes make the corpus reproducible.

Focused integration tests cover both compiler optimization modes and injected
allocation failures, preserving both borrowed operands. After the focused
checks and self-review/fix cycle, `RUSTC_WRAPPER= just test` passes: 318 unit
tests, two binary tests, 6,911 integration tests (four ignored), nine
compile-benchmark tests, 304 runtime tests (five ignored), and both doc-test
stages. The saved report is
`target/check-timings/suite-20261115T050240-1905838/`: 73.8 seconds compile,
291.7 seconds execution, 365.6 seconds total. This is qualification timing,
not an uncontended compiler-performance comparison.

All-target/all-feature workspace Clippy with `-D warnings`, formatting and
whitespace checks pass. All three docs CI commands pass: 814 Markdown files
and 6,103 local link targets, 387 Python tests (four skipped), and the complete
pinned-mdBook site with 1,652 default and 826 dev HTML pages.
No peak-memory/allocation-count, other CPU architecture, runtime-checks
performance, or cross-machine crossover claim is made.

## Reproduction and retained artifacts

Build the comparator using the locked experiment manifest and compile
`experiments/bigint-comparison/bench.plenty` with an optimized Plenty compiler.
Keep the baseline compiler/executable separate; a current rebuild is not a
baseline. Generate the original inputs with `compare.py generate --out DIR`.
The final measurement command is:

```sh
python3 experiments/bigint-comparison/algorithms-next/measure.py \
  --baseline BASELINE --candidate FINAL --malachite MALACHITE \
  --inputs DIR/benchmark-inputs.json --out RESULTS \
  --kernel-c KERNEL_C --kernel-plenty KERNEL_PLENTY --compiler COMPILER
python3 experiments/bigint-comparison/algorithms-next/constants.py
python3 experiments/bigint-comparison/algorithms-next/oracle.py \
  --compiler COMPILER --malachite MALACHITE --out ORACLE
```

[Metadata](results/metadata.json) records executable/source SHA-256 hashes,
toolchain, host and CPU affinity. Durable raw observations, all labelled phase
binaries, exact final compiler/API/kernel executables, source snapshots,
oracle inputs/outputs, assembly/CLIF and qualification logs are retained under
the primary worktree's `target/issue-987-algorithms-next-evidence/` with a
SHA-256 manifest. The earlier baseline/Malachite provenance archives remain
unchanged.
