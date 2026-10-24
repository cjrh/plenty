# Portable SIMD without everyday SIMD bookkeeping

> **Status: Initial slices implemented; broader design remains proposed.**
> The [explicit module](../design/stdlib/simd.md) now supplies concrete portable
> lanes, masks, scalar fallback and bounded native lowering. See its
> [implementation evidence](std-simd/implementation.md) for API differences and
> representation limits. This proposal for B23 was researched against Plenty
> `7654ecca` on 2026-10-23. The [current pipeline contract](../design/language/09-iteration/06-pipelines.md)
> implements serial fusion and bounded scalar callback substitution. The first implemented automatic SIMD
> slice is [integer-column filter/count](columnar-list/simd-counts.md); the broader
> API extensions and recognition patterns below remain proposals.
> This refines the SIMD portion of the earlier
> [memory and parallelism proposal](memory-parallelism-and-simd.md).

## Recommendation

Give ordinary numeric code SIMD benefits without making its author select an
instruction set, vector width, mask layout, alignment, or tail strategy. Introduce
two cooperating facilities:

1. A small portable `std.simd` module for algorithms that genuinely need explicit
   lanes, comparisons and selection. Follow Rust's portable *semantics and value
   model*, not its entire trait, pointer and intrinsic surface.
2. A bounded compiler transformation for recognizable fluent pipelines. Prove
   that a transformation preserves the existing scalar program; otherwise keep
   the scalar program. No `.simd()` marker or purity assertion should be required
   for the safe common cases.

An ordinary pipeline should be the recommended starting point. Explicit SIMD is
an advanced tool, not a prerequisite for efficient arithmetic. Compile-time
diagnostics can explain missed opportunities without making correctness depend
on which optimizer or CPU is selected.

The [API contract](std-simd/api.md) places bounds, alignment and representation
complexity in the library/compiler. The [automatic transformation design](std-simd/automatic.md)
states exactly what the compiler must prove. The [spike evidence](std-simd/evidence.md)
includes reproducible native tests and backend comparisons.

## Portability is a semantic promise

Public names describe lanes and operations, never SSE, AVX or NEON. A fixed
logical vector can lower to one instruction, multiple narrower instructions,
or scalar operations. The chosen lowering must preserve the same lane results,
bounds behavior and arithmetic failures. “Portable” does not mean every target
has equal speed or an implemented Plenty runtime today.

Start with tested 128-bit logical shapes and a scalar implementation of every
published operation. Keep masks opaque and vector layout out of the public ABI.
Avoid arbitrary lane-count generics, scalable-vector types, native register
exposure and CPU multiversioning in the first API. Those are separate design
choices, not hidden prerequisites.

Contiguous numeric storage matters more than vector syntax. Existing generic
lists are not promised to be packed arrays of lane values. Do not silently copy
them into scratch arrays to claim automatic SIMD. Counted ranges can benefit
without new storage; packed numeric buffers/views need a deliberate B12/B19
contract before direct vector loads become an everyday list optimization.

## Relationship to automatic parallelization

Both transformations need source bounds, visible callback bodies, effect
summaries, and a cost model. Share that analysis and explanation machinery with
B24 and the [fluent pipeline design](fluent-iterators.md), but keep the decisions
separate. SIMD runs on the current thread and does not introduce scheduling,
thread transfer, cancellation, synchronization or thread-pool overhead.

“Serial” continues to mean serial observable semantics: the implementation may
evaluate independent, total scalar operations in vector lanes. This clarifies
the earlier proposal's “always serial” wording without enabling implicit
parallel workers. Existing explicit pool operations retain their own ordering
contract; vectorizing inside a worker is an independent later opportunity.

## Cranelift or LLVM?

Keep the API and candidate analysis backend independent. The spike shows that
Cranelift can generate useful 128-bit SIMD for x86-64 and AArch64 when supplied
vector IR. It also shows real limits: direct 256-bit `i32x8` was rejected on
both tested targets, scalar CLIF loops stayed scalar, and LLVM produced materially
better code for a simple constant-multiply kernel. See the evidence page for
measurements, tool versions and the limits of that comparison.

Cranelift therefore does not block the initial facility, but fast compilation
alone is not a reason to accept persistently poor hardware utilization. Do not
build a growing general optimizer in Plenty just to avoid evaluating LLVM.
Use these adoption gates before a production implementation commits to a backend:

| Evidence | Decision it should trigger |
| --- | --- |
| Bounded recognition plus 128-bit lowering reaches useful speedups at low compilation cost | Ship a narrow Cranelift path with scalar fallback |
| A small, semantics-preserving local rewrite closes a measured gap | Add it only with differential tests and a bounded complexity budget |
| Important workloads need wider/scalable vectors, gathers, sophisticated dependence analysis or repeated backend-specific rewrites | Prototype the same typed kernel representation through LLVM; reconsider backend timing |
| LLVM remains substantially faster across representative application kernels after equivalent arithmetic and alias contracts | Consider an optimizing build mode or an earlier primary-backend switch |
| LLVM benefits are isolated, compile latency or packaging costs dominate | Retain Cranelift and explicit specialized kernels; do not maintain two full backends by default |

For the decision, measure cold and incremental end-to-end compilation, optimizer
time, executable size, runtime and memory on multiple input sizes and targets.
Include bandwidth-bound, reduction, branchy/filter, and small-input workloads.
Compare equivalent safety checks, target features, alias assumptions and floating
semantics. This spike is a reason to investigate LLVM, not a migration benchmark.
The earlier [GCC backend investigation](codegen-gcc.md) is related context, not a
dependency of either SIMD approach.

## Implementation boundaries and acceptance

B23 in the [backlog](../backlog.md) owns prioritization. A sensible dependency
order is a private lane-value representation with scalar oracle, the small public
module, then narrowly recognized automatic candidates. Range reductions can be
investigated alongside the module; neither depends on automatic threading.
Packed-buffer algorithms depend on the storage contract, not on finishing all
allocator work.

Before adoption, require native/scalar differential tests, target capability
tests, tail and alignment checks, preserved diagnostics, and code-generation
evidence. Run the AArch64 cases on hardware before claiming execution support.
Teach ordinary pipelines first when the transformation ships, then show explicit
vectors only where they clarify a real algorithm. Until then, keep these sketches
out of the reference, signature index and executable tutorial.
