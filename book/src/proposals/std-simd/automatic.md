# Automatic SIMD for recognizable pipelines

Part of the [portable SIMD proposal](../std-simd.md). This is a bounded proposed
transformation, not implemented auto-vectorization. Fluent pipelines already
preserve source and callback structure; the compiler should use that information
before it disappears into low-level control flow.

## Start with code people already write

These are current Plenty spellings; the proposed optimization changes only
their implementation:

```text
total = iter(range(0u32, 1000u32)).map(
    def(x: u32) -> u32: x.wrapping_mul(3u32).wrapping_add(7u32)
).fold(0u32, def(acc: u32, x: u32) -> u32: acc.wrapping_add(x))

even = iter(range(0u32, 1000u32)).filter(
    def(x: &u32) -> bool: x & 1u32 == 0u32
).count()
```

The first has a pointwise modular map and associative modular reduction. The
second needs a comparison mask and a count, not a compacted intermediate list.
Neither needs heap allocation, a thread pool, or a new annotation. The checked
`i64` count is safe only when the source bound proves it fits. Guarded induction
must avoid overflowing while constructing the final vector of range values.

| Pattern | Admission rule and initial disposition |
| --- | --- |
| Sized range → total scalar maps → wrapping integer fold | Recognize exact modular reduction; vector accumulators plus ordered scalar remainder |
| Sized range or packed read-only view → bitwise/comparison filter → count | Mask count; bound the result to `i64`; no materialized filtered collection |
| Packed numeric input → total pointwise map → distinct preallocated destination | Vector loads/stores once views and exclusive destination proofs exist |
| Equal-domain zip of packed sources → pointwise map | Later bounded extension; preserve left-first consumption and prove both lengths before evaluation |
| `skip`/`take` on a sized pure source before maps | Normalize bounds only when it preserves evaluation and count rules |
| Floating left fold, ordinary checked integer fold | Preserve order; exclude reassociation without a proof or a separately specified operation |
| `filter(...).map(...)`, `filter(...).collect()`, `take_while`, `any`, `find` | Defer: evaluation suppression, compaction or early stopping needs additional reasoning |
| Generators, indirect calls, I/O, mutable captures, allocation, ownership cleanup, atomics | Scalar fallback in the first implementation |

A recognized floating map can preserve each element's multiply-then-add order;
a floating reduction cannot freely change its accumulation tree. Even ordinary
integer addition is not freely associative under overflow trapping: a sequential
prefix may overflow although a regrouped sum fits. Do not infer safety from a
final result bound alone.

## Proof before profitability

For each candidate establish all of these facts:

1. **Source:** finite length, element type, traversal order and storage stride
   are known. Prove each full-block access lies within the source; use scalar
   tails. No speculative reads into a later page or adjacent object.
2. **Body:** a bounded visible expression graph uses only allowed total scalar
   operations and immutable scalar captures. Reject unknown calls rather than
   trusting a user purity assertion. Purity alone does not establish totality.
3. **Effects:** no observable callback order, writes through aliases, allocation,
   cleanup or failure can move across a block boundary. Preserving only the
   successful result is insufficient.
4. **Terminal:** a specific reduction law or destination layout is known.
   A function named `add` is not evidence of associativity. Match typed operations
   after substitution; retain scalar fallback for arbitrary `fold` callbacks.
5. **Target and cost:** every widened operation has legal lowering or a useful
   fallback. Trip count, load stride, scalar remainder, instruction expansion and
   code size justify the transformed loop.

Known-length `collect` is not automatically admitted. It allocates, may reserve
or grow, and has ownership/failure behavior. First implement preallocated numeric
destinations or nonallocating reductions. Later preserve the existing reservation
point and capacity policy before changing its inner loop; do not introduce an
infallible hidden temporary array.

Checked element maps could later use a vector failure mask and replay the first
failing block scalarly, but only when replay has no effects and reproduces the
first source operation and item that fail. This is substantially harder than
wrapping arithmetic and is not an initial fast path. Never run speculative
trapping instructions and then attempt to recover after selection.

## Placement in the compiler

Current `src/frontend/pipelines.rs` builds an `Iteration` body while adapting a
pipeline; it does not retain an optimizer-ready list of typed stages. Extend the
plan with bounded source/adapter/terminal metadata instead of reconstructing
source expressions from arbitrary CLIF loops. Keep a scalar lowering as the
semantic oracle. Preserve ownership, loan and cleanup validation before any
optimization decision.

Small callback substitution already exists in `src/codegen/leaf_calls.rs`,
`small_calls.rs` and `scalar_captures.rs`. Reuse its bounded body visibility,
but do not confuse “eligible for substitution” with “nontrapping and safe to
speculate.” A separate whitelist/effect summary must certify widening. Analysis
may happen after specialization, with plan identity and source sites retained
through substitution. Cache keys must include the transformed plan, target
features and optimization policy.

Lower an admitted plan into a backend-neutral kernel description: logical lane
operations, source bounds, output/reduction policy, scalar tail and original
sites. Cranelift can receive explicit vector IR; LLVM can receive equivalent
scalar loops with truthful alias and arithmetic facts, or explicit vectors.
No `noalias`, `nsw` or `nuw` annotation may be guessed to coax better code.

Keep the analysis bounded by chain/body size; do not add whole-program alias
analysis or arbitrary loop-dependence solving as a prerequisite. Share summaries
with automatic parallelization, but neither optimization grants the other's
safety proof.

## Make behavior easy to understand

Optimization misses are not errors. Offer an opt-in report with the source
span and one actionable reason, for example:

```text
SIMD: map/filter-count uses four lanes; scalar tail for remaining items
SIMD skipped: this callback can trap on integer overflow
SIMD skipped: ordered floating fold must preserve accumulation order
SIMD skipped: source layout does not provide contiguous numeric lanes
SIMD skipped: seven items are below this target's profitable threshold
```

The report and a compiler option to disable automatic SIMD are debugging tools,
not new language semantics. Do not require users to read IR or choose an ISA to
understand a missed optimization. Explicit `std.simd` remains available where an
algorithm needs lane control, with the same portable contract and scalar fallback.

Acceptance includes empty and short inputs, every remainder, range boundaries,
unaligned views, overflow and failure ordering, NaNs and signed zero, callbacks
with effects, zip length mismatch, scalar fallback, and disabled-optimization
equivalence. Correct results alone are not evidence of SIMD: inspect generated
instructions and benchmark enough sizes to reveal overhead and memory limits.
