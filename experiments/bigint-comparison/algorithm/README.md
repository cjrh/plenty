# Exact small-prime multiplication investigation (#987)

This revision extends local PR #1279 after its measured baseline commit
`6e88cc39c9e1053843b49d85e92103d0c7afeec2`. That commit's scalar tuple lowering,
borrowed Karatsuba operands, direct result accumulation and shared radix powers
remain in the combined PR. This directory records the subsequent algorithm
investigation and independent Plenty implementation. Issue #987 remains open.

## Actual Malachite 0.14 paths

The comparator locks Malachite 0.14.0. Its cached crate archive has SHA-256
`7ac55a9e8426447c374668b93c35b7b6ab43a21dc7ba97ad0e852be2305a5900`, matching
Cargo.lock. All 2,588 extracted source files were byte-compared against that
archive. [Source provenance](source-paths.json) and the reproducible
[source dispatch/profile proof](paths.py), with [output](path-proof.json),
identify the actual x86-64 branches rather than assuming every named algorithm
is used. Limbs are u64, with Small(Limb)/Large(Vec<Limb>) magnitude storage and
a separate integer sign.

| Retained operand bits | Limbs each | Malachite multiply | Plenty before this revision | Final Plenty |
| --- | ---: | --- | --- | --- |
| 16 | 1 | inline limb multiply | inline i64 | inline i64 |
| 256 | 4 | basecase | schoolbook | schoolbook |
| 2,048 | 32 | Toom-2.2 (Karatsuba) | Karatsuba | Karatsuba |
| 8,192 | 128 | Toom-3.3 | Karatsuba | Karatsuba |
| 65,536 | 1,024 | small-prime FFT | Karatsuba | two-prime NTT/CRT |

The equal-size dispatcher uses Toom-2.2 at 16, Toom-3.3 at 105, Toom-6h at
300, Toom-8h at 389 and **FFT at 1,000 limbs**. Its Toom-4.4 band is empty
because its threshold equals Toom-6h's. The unequal dispatcher also selects
FFT when the shorter input reaches 1,000 limbs; the older general dispatcher
contains a distinct MUL_FFT_THRESHOLD=389, which must not be mistaken for the
active equal-size boundary. Retained 32x64, 64x32, 65x32 and 65x33 limb shapes
select Toom-4.2; 128x2 selects basecase. Malachite Toom-2.2 uses evaluations
0,-1,infinity and absolute differences, avoiding the extra sum limb. Plenty
uses 0,+1,infinity, borrowed recursive slices and a 32-limb threshold.

Aliased `&Natural * &Natural` selects squaring, with separate thresholds:
35 for Toom-2, 67 for Toom-3, 285 for Toom-6 and 389 for FFT. Thus the retained
4/32-limb aliased squares use basecase, 128 uses Toom-3 and 1,024 uses FFT.
The timing harness parses two independent operands and does not take that
pointer-alias square path, even when values match. Plenty has no square
specialization.

Malachite's FFT is the Daniel Schultz/FLINT small-prime modular transform and
CRT approach, using primes below 2^50, floating-point modular arithmetic,
AVX2/FMA dispatch, truncated radix-4 transforms, borrowed precomputed context
and reusable scratch. This is not a claim that Schonhage-Strassen executes.
Source-derived profile selection for our 1,024-limb fixture chooses six
primes, 144-bit chunks, 456 input chunks, 911 output coefficients, 1,024
transform slots and forward input truncation to 512. Our two-prime, 16-bit
packing needs 8,192 slots: full butterfly work alone is about 3.47 times
larger, before vectorization, truncation and generated-code costs.

## Implemented mathematical approach

The production module directly implements exact radix-2 NTT convolution and
CRT in Plenty. No Malachite/FLINT code or tables were copied, no LGPL code
ships, and no production native dependency was added. The source was studied
to determine control flow and algorithm choice; the implementation follows
the mathematical specifications in the reference links below.

Inputs are split into base-2^16 digits. Primes 998244353 and 1004535809 share
primitive root 3 and support transform lengths through 2^21. The maximum
convolution coefficient is below 2^20*(65535^2), far below their product;
CRT therefore recovers the integer coefficient exactly, then a radix carry
pass packs u64 limbs. [constants.py](constants.py) verifies prime/root/CRT
constants and 20,000 independent modular arithmetic cases; its
[proof output](constant-proof.json) records the bounds.

DIF forward and DIT inverse transforms avoid bit reversal. Roots and their
Shoup factors share one allocation. A fixed-twiddle multiplication uses
`q=(value*floor(root*2^32/p)) >> 32`, followed by `value*root-q*p` and at most
one correction. Montgomery R=2^32 arithmetic handles root recurrence and
pointwise multiplication. Forward data use ordinary residues; the inverse
scale cancels both the transform length and the pointwise Montgomery factor.
All wrapping arithmetic has explicit modular/bounded integer justification.
Four fallibly allocated arrays are reused between primes, with borrowed
blocks/slices inside the butterflies. Public errors, sign/floor conventions,
inline i64 normalization, carry correctness and operand preservation remain
covered by integration tests.

Dispatch requires both operands to contain at least 1,024 u64 limbs, at most
a 2:1 size ratio, a combined length at most 524,288 limbs, and at least 7/8
occupancy of the next power-of-two transform. Other products retain existing
Karatsuba/schoolbook dispatch; recursive children can independently qualify.
The transform ceiling is an implementation crossover limit, not a new
BigInt precision limit.

## Final measurements

The optimized default compiler and identical API harness were frozen before
measurement. [Metadata](results/metadata.json) records exact executable
hashes, host/toolchain and CPU-0 affinity. [Paired instruction counts](results/instructions.csv)
use three N/2N differences, removing process/setup intercepts; [CPU samples](results/cpu.csv)
use five alternating batches. These are host-specific observations, not a
cross-machine crossover guarantee. All raw stdout/stderr and executable
artifacts are in the durable archive named below.

| Shape (u64 limbs) | Prior PR instructions | Final instructions | Prior CPU ms, median [min,max] | Final CPU ms, median [min,max] |
| --- | ---: | ---: | --- | --- |
| 1024 x 1024 (65,536 bits) | 59,057,242 | 40,838,813 | 3.519 [3.379,3.551] | 3.257 [3.220,3.378] |
| 1536 x 1536 (fallback) | 91,323,172 | 91,334,831 | 5.956 [5.743,6.000] | 5.908 [5.796,6.108] |
| 1792 x 1792 | 108,467,548 | 82,599,751 | 7.375 [7.125,7.426] | 7.040 [6.836,7.181] |
| 1200 x 2384 | 142,340,689 | 82,605,388 | 8.729 [8.426,8.816] | 6.785 [6.667,7.184] |
| 2048 x 2048 | 179,773,664 | 84,343,462 | 10.410 [10.271,10.711] | 6.962 [6.863,7.486] |

The first row uses the original 65,536-bit fixture; the supplemental
1024x1024 fixture is separately repeated in the CSV. Instruction improvements
are 1.45x and 2.13x at 65,536 and 131,072 bits; CPU median improvements are
7.4% and 33.1%. The 1792-limb CPU ranges overlap, so its modest gain is
host-specific. Malachite needs 887,070 and 1,781,978 instructions respectively,
leaving **46.0x and 47.3x gaps**. Medium Toom-3, packed/truncated/vectorized
transforms, square specialization and representation/helper costs remain.

The CSV deliberately retains all 25 original API workloads and padding,
ratio and size controls. Non-transform results show layout/timing variation:
for example 1024x2049 and 2049x1024 keep schoolbook but have 3.8%/7.4% slower
CPU medians, with overlapping ranges. There is no claim that every workload
improves. Addition/division/parse/render instruction behavior is essentially
unchanged by this algorithm revision; prior PR costs/regressions remain in
[the earlier report](../performance/README.md).

A previous all-Montgomery butterfly candidate reduced instructions but lost
CPU time at 65,536 bits and at 75% occupancy. Its [retained measurements](mont-results/cpu.csv)
and [instruction counts](mont-results/instructions.csv) explain the final
Shoup and 87.5% choice. Its full-suite run was interrupted intentionally
(exit130, suite-20261115T033131-41268); it is not a passing validation of the
final implementation. Pilot and obsolete binaries remain labelled in the
archive, outside production dispatch.

## Core and generated-code attribution

[core.plenty](core.plenty) runs the same complete two-prime multiplication on
preallocated inputs/output/scratch. Its paired count is 21,889,008 instructions
per product, compared with 40,838,813 for the public BigInt API. The 18.95M
difference includes owning representation/storage, allocation and different
inlining context; this experiment does not isolate an allocation count or
measure peak memory. No allocation totals or peak-memory claim is made.

The isolated 8,192-point forward [Plenty kernel](kernel.plenty) and checked
[C kernel](kernel.c), both using the same Shoup butterfly, require 3,006,638
and 1,786,013 instructions respectively (1.68x). Both include runtime checked
arithmetic and indexing. The [Plenty disassembly](kernel-final-plenty.asm)
and [C disassembly](kernel-final-c.asm), with the loop-counter outputs in this
directory, give 52 versus 32 static instructions in the main butterfly loop.
Plenty repeats data slice bounds checks, checks the second root-half index,
computes its offset and checks signed index additions, retains overflow
branches after modular correction, and spills/reloads a loop step. GCC
combines data addressing, proves the fixed root-half offset and uses
conditional moves for corrections. These account for the remaining loop
shape difference; static loop size is not a dynamic instruction attribution.
No further compiler change is made on the strength of this experiment.

## Correctness and reproduction

The final 216-case large-product corpus compares Python exact arithmetic,
Malachite 0.14 and Plenty, including random/all-ones/power-of-two inputs,
four sign combinations, threshold/padding boundaries, 2:1 and just-over-2:1
operands. [Its summary](results/oracle-summary.json) records exact hashes.
The existing 195-case/16,378-line oracle additionally covers all radices,
rounding and signed floor division. Focused bigint tests include allocation
failure budgets, unchanged operands and assignment, carries and sparse
squares. The final full suite passed: 6,909 integration tests (4 ignored), 318 compiler
tests, 304 runtime tests (5 ignored), nine compile-benchmark tests and two
binary tests. Its saved report is `suite-20261115T034548-758116`, 35.1 seconds
compilation plus 290.8 seconds execution. All-features Clippy, formatting and
all three docs checks passed. Combined cache fixtures with ccache first in
PATH passed all 29 tests through the exact final test binary; an earlier
sandboxed Cargo attempt failed before tests on a read-only user ccache path.
Both logs are retained, with the initial attempt labelled accurately.

Run `python3 paths.py --help`, `python3 measure.py --help` and
`python3 oracle.py --help` from this directory for explicit input/output
paths. Use the archived baseline and final executables, not a rebuilt program
with different feature flags. Kernel C uses `gcc -O2 -fno-pie -no-pie`;
Plenty uses the optimized default compiler. Benchmarking must use a quiet
CPU window; broad tests were run afterwards.

The durable local evidence archive is
`/home/caleb/Documents/repos/plenty/target/issue-987-algorithm-evidence`.
The earlier exact baselines remain in `target/issue-987-evidence` and
`target/merged-pr-evidence/pr-1278` in the primary worktree. The final archive
contains compiler/executable hashes, raw counters/timing samples, corpora,
outputs, source/profile proofs, generated assembly and validation logs,
including the interrupted obsolete suite.

## References

- [FLINT multimodular multiplication](https://flintlib.org/doc/algorithms/fft/multimodular.html): mathematical packing, modular transforms and CRT; source behavior was checked independently.
- [FLINT small-prime FFT](https://flintlib.org/doc/algorithms/fft/small-prime-fft.html): the algorithm family actually used by Malachite.
- [Shoup modular multiplication](https://flintlib.org/doc/algorithms/arithmetic/shoup.html): fixed-multiplier reduction; its bound is independently checked here.
- [Montgomery, Modular multiplication without trial division (1985)](https://doi.org/10.1090/S0025-5718-1985-0777282-X): exact integer modular multiplication.
- Malachite 0.14 source, identified by the locked archive checksum above: dispatcher and profile investigation only; LGPL source was not copied into Plenty.
