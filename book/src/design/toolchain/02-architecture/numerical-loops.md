# Small numerical loop state

Optimized builds can keep the cells of a small numerical list, including fields
of `@copy` records and the columns of a [`clist`](collections.md), in scalar
SSA variables across a counted outer loop. The
compiler expands bounded inner `range` loops and replaces constant-index reads
and writes with those variables. It loads state on entry and writes modified
cells back on exit. The original n-body benchmark benefits without a source
rewrite; this transformation does not require SIMD syntax or a new array type.

`src/codegen/fixed_state.rs` runs after
[call-site substitution](emission.md#call-site-substitution), before tail-call
planning and last-use moves. Its replacement operations enter the
[function cache key](cache.md). `--fast-compile` retains the original operations.

## Hints and guards

A literal list capacity between two and eight, or such a literal passed to
`clist[T].with_capacity`, supplies a speculative size hint
for that exact numerical list type within the compilation unit. A numerical
list type is a list of `f32` or `f64`, a list of a `@copy` class whose fields
are all `f32` or `f64`, or a clist whose row fields are all `f32` or `f64`; a
literal capacity of any other type is ignored. Conflicting
literal capacities disable that type's hint. A function is considered only
when one of its parameters or locals has a hinted type. A capacity is **not** evidence of
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

Only `f32`/`f64` cells are promoted: an element of a float list, a float field
of a `@copy` record in a list, or one row of a float column of a `clist`
(issue #930). The other fields of the record or row class do not matter to
this: the float columns of a row that also has integer or `bool` columns are
cells once the function has a hinted list, although such a row type supplies
no hint itself. A column or field of another type is never a cell, and a loop
that reads or writes one is left as written. Clist rows are values stored
in the columns, so no instance is shared between two rows or with another
variable, and the cells of one clist are distinct memory.
There may be one borrowed list root and distinct owned list roots; local
reference aliases and multiple borrowed roots are rejected. Two owned
lists are different allocations, and a borrowed parameter cannot refer to a
list the function owns, so no two promoted roots share a cell. Inside the loop,
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
A clist with a row has an allocation, so its guarded accesses also skip the
test for an unallocated clist.
It also preserves scalar assignments that can be
observed after the loop.

## Paired statements

On x86-64, optimized builds compute adjacent statements of one shape in the two
lanes of an `f64x2` value. The expansion above leaves such statements in a row:
n-body's magnitude for one pair of bodies, then the same expression for the
following pair. The lowerer packs `2n` statements, `n` at most 8, when:

- each is an expression over `f64` locals, `f64` constants, `+`, `-`, `*`, `/`
  and `sqrt`, stored to an `f64` local;
- statement `i` and statement `n + i` differ only in the locals they name;
- the first `n` contain a division or square root, the operations that cost
  enough to repay building and splitting a vector;
- the second `n` read nothing the first `n` write, so computing them side by
  side reads the same values.

Each lane of a vector `fadd`, `fsub`, `fmul`, `fdiv` or `sqrt` is the IEEE 754
operation of the scalar instruction, and the operations of a lane keep their
order. Every stored value therefore has the bits the scalar statements store,
including infinities, NaNs and subnormals. None of these operations can fail,
so no trap or error moves. Integer constant assignments between the statements,
which the expansion uses for its loop counters, run after the lanes in their
original order: they read nothing and write no `f64` local.

Only expanded code packs today. Statements written in source carry error-site
and access markers between them, which the matcher does not cross. Addition,
subtraction and multiplication alone are left scalar, because lane inserts and
extracts cost more than they save. Vectors wider than two lanes are not formed.

This is a deliberately narrow optimization. It does not imply general aggregate
scalar replacement, general automatic vectorization, or expansion of arbitrary loops.
Measurements and remaining scope are linked from the
[performance record](../../../completed-work/performance.md#guarded-numerical-state-issue-930).
