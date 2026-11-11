# Small numerical loop state

Optimized builds can keep the cells of a small numerical list, including fields
of `@copy` records, in scalar SSA variables across a counted outer loop. The
compiler expands bounded inner `range` loops and replaces constant-index reads
and writes with those variables. It loads state on entry and writes modified
cells back on exit. The original n-body benchmark benefits without a source
rewrite; this transformation does not require SIMD syntax or a new array type.

`src/codegen/fixed_state.rs` runs after
[call-site substitution](emission.md#call-site-substitution), before tail-call
planning and last-use moves. Its replacement operations enter the
[function cache key](cache.md). `--fast-compile` retains the original operations.

## Hints and guards

A literal list capacity between two and eight supplies a speculative size hint
for that exact numerical list type within the compilation unit. Conflicting
literal capacities disable that type's hint. A capacity is **not** evidence of
initialized length, and no function name or benchmark-specific size is built
into the pass.

A conservative scan propagates hints through local integer calculations. At an
eligible outer loop, the compiler produces one guarded version and keeps the
original loop as the fallback. Entry checks require:

- A positive outer range count, so empty ranges do not access state.
- Equality for every retained speculative integer fact used by the expansion.
- Sufficient actual length for every accessed list cell.
- The exact length of every list whose length the expansion used.

A `range` loop with a constant count from 1 to 64 that appends once per pass
to an owned list suggests that list's length; several such loops add up. The
suggestion is only a guess: the expansion may use it to evaluate
`len_*`/`tail_*` of a `std.simd` view or `len` of a view, and the guard then
requires that exact length.

The size guard can fail after a caller appends elements, or when a scratch list
has fewer initialized rows than its capacity. Failed guards execute the original
loop, including its original checks, source positions and errors. Allocations
and initialization preceding the loop remain in their original order.

List header reads that a loop nest cached immediately before the candidate
loop (see the [collection architecture](collections.md)) belong to the
fallback alone (issue #943). The guard reads each length from the list, the
guarded version uses no cached header, and nothing else in the function may
read those locals. A header read ahead of the guard would stay live across the
guarded loop, whose promoted cells already exceed the registers. On the
fallback path each read runs before the outermost loops that use it directly,
as it did before the nest shared its caches. That placement needs every nested
loop body to have been evaluated, which shows that no operation of the loop
resizes or rebinds a list; when an inner loop ran no iteration under the
guarded facts, or the loop writes the cached local, the read runs once at the
start of the fallback instead.

## Eligibility and limits

Only `f32`/`f64` cells and fields of copyable numerical records are promoted.
There may be one borrowed list root and distinct owned list roots; local
reference aliases and multiple borrowed roots are rejected. Inside the loop,
recognized work consists of scalar floating arithmetic and square root,
constant-proven integer calculations, wrapping range bookkeeping, and accesses
to constant nonnegative indices. The outer range count must be dynamic at this
pass; it stays a loop. Inner loop conditions and iteration counts must
be computable under the guarded facts.

Floating `std.simd` [view accesses](../../stdlib/simd.md#indexed-access-through-views)
on a view of all rows of a promoted list take part (issue #929). `get_*`
builds its vector from the lanes' scalar variables and `set` splits a vector
into them, so scalar and vector accesses to one row share a variable and no
vector load waits on a scalar store. Lane arithmetic, `sqrt` and `splat_*`
are values of their operands. The guard proves every vector's rows; an index
it cannot prove, a negative index or a view of part of a list prevents the
transformation, and the original loop keeps each access's trap. The
`load_*`/`store` transfers, integer vectors, masks and views received as
parameters are not recognized.

Other calls, allocation, resizing, escaping references, unsupported control flow and
integer arithmetic that might fail prevent the transformation. This ensures
that delaying writes until loop exit cannot expose intermediate state to another
operation or change what a failure observes. Floating expressions retain their
original order; no reassociation, approximate arithmetic, or fusion is added.
Cached list views must come from the same root as their accesses.

Each function gets at most one version, bounded by 128 expanded inner iterations,
12,000 emitted operations, 96 promoted cells, and the existing 256-local limit.
Candidate functions with more than 220 existing parameter/local slots are skipped.
The pass accepts indices below 64. The dominating length guards prove entry
loads and exit stores in bounds, so those accesses need no duplicate checks.
It also preserves scalar assignments that can be
observed after the loop.

This is a deliberately narrow optimization. It does not imply general aggregate
scalar replacement, automatic SIMD packing, or expansion of arbitrary loops.
Measurements and remaining scope are linked from the
[performance record](../../../completed-work/performance.md#guarded-numerical-state-issue-930).
