# Why the integer multiply probe is slower

The [initial module](implementation.md) keeps the recurrence in vector registers,
but baseline x86-64 lacks a single instruction for all four low 32-bit products.
The original probe compares four independent scalar accumulators with one vector
accumulator. Both compute exactly the same four modular multiply/add recurrences.

## What actually executes

Optimized CLIF contains `imul.i32x4` followed by `iadd`. Cranelift 0.137.0's
`isa/x64/lower.isle` (lines 1168–1183, source commit
`ebe5e8df1f4e12c5fc2f5954926881d8d61367d8`) selects this baseline sequence:

```text
pshufd      value, odd-lane-order
pshufd      factor, odd-lane-order
pmuludq     original-value, original-factor
pshufd      even-products, low-halves
pmuludq     odd-values, odd-factors
pshufd      odd-products, low-halves
punpckldq   even-low-products, odd-low-products
paddd       increment, result
```

The scalar loop has four independent `imul` plus `add`/`lea` pairs. The vector
loop has shuffle, multiply, shuffle, interleave and add dependencies before its
next iteration. Similar instruction totals therefore do not imply similar
critical paths. The hot loop has no tagged-lane gathering or scattering. Its
one invariant stack load is bookkeeping; canonical vector construction and
final extraction are outside the loop.

An experimental compiler enabled **only** `has_sse41`; the production compiler
still uses its existing baseline. The same CLIF then becomes `pmulld` plus
`paddd`. Intel documents `PMULLD` as the SSE4 operation retaining each product's
low 32 bits; Rust's matching intrinsic requires SSE4.1. See the
[Intel intrinsic reference, page 136](https://www.intel.com/content/dam/develop/external/us/en/documents/18072-347603.pdf)
and [Rust intrinsic implementation](https://doc.rust-lang.org/stable/src/core/stdarch/crates/core_arch/src/x86/sse41.rs.html).

## Same workload, controlled comparisons

Final measurements on the Ryzen 7 7840HS use one million iterations per timing
sample and ten million for counters. The numbers below are median user-space
cycles per iteration from three counter runs, including small process overhead;
they are observations for these loops, not instruction-latency specifications.

| Kernel | Scalar | Baseline SIMD | Experimental SSE4.1 SIMD |
| --- | ---: | ---: | ---: |
| Four lanes, constant factor | 4.08 | 7.12 | 4.06 |
| Four lanes, runtime factor | 4.07 | 7.11 | 4.05 |
| Eight lanes, two independent vectors | 8.09 | 8.17 | 4.27 |

The runtime-factor variant removes scalar immediate-multiply specialization;
its result is essentially unchanged. Two independent vectors hide more of the
dependency cost: baseline throughput reaches scalar parity, and SSE4.1 is about
1.9 times faster. A single SSE4.1 vector only reaches parity with four scalar
chains. Fewer instructions alone are not a speedup guarantee.

Instructions per iteration for the runtime-factor case are approximately
13/14/8 for four lanes and 21/22/10 for eight lanes (scalar/baseline/SSE4.1).
All variants check identical modular results. Reproduction sources, raw samples,
counter output and load context live in the repository's
`experiments/simd/explicit/` directory; start with its `README.md`.

## The original exit-shape spill and its fix

The first eight-lane variant finished with
`v0.wrapping_sum().wrapping_add(v1.wrapping_sum())`. Its generated loop spills
one vector with a `movdqu` load/store pair, costing about 16.2 baseline cycles
or 13.0 SSE4.1 cycles. Changing only that final expression to
`v0.wrapping_add(v1).wrapping_sum()` removes the spill and yields the table above.
This rearrangement is valid for modular addition; it is not a license to
reassociate checked or floating arithmetic.

The spill-free loop uses only six XMM registers. Issue #444 traced the important
distinction: the second vector remained live across the first reduction's scalar
ABI call and caller-location hooks. On the tested System V target, XMM registers
are caller-clobbered, and the
allocator placed that vector's spill across the loop. The combined reduction
had no second live vector at its single reduction call.

The fix lowers the existing integer `wrapping_sum` directly to lane extraction
and modular additions. It removes the reduction calls and their argument
materialization, so the original separate-sums spelling also keeps both vectors
in registers. It does not change floating reductions, arithmetic order, ABI or
ISA policy, or claim to fix general register-allocation heuristics. The
`integer_spill.plenty` probe retains the original source shape; before/after
measurements are in `spill-measurements.json` in the experiment directory.
With the baseline ISA unchanged, the separate-sums kernel improves from about
16.2 to 8.1 cycles per iteration (3.369 to 1.676 ms per million iterations),
reaching scalar parity. The single-vector multiply dependency cost remains.

The recommendation is to accept the portable baseline with these measured
limits. The original slowdown is explained by its instruction set and dependent
chain, rather than lingering tagged storage in that hot loop. Newer target
selection can improve throughput, but requires a separate explicit compilation
or dispatch policy. It should not be silently enabled to make this benchmark win.
