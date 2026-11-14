# Declared-vector evidence and prototype gates

**Approved investigation plan for unimplemented [declared vectors](../declared-vectors.md).**
No new benchmark or prototype was run for this proposal. The figures below are
previously recorded evidence, not measurements of the candidate and not a
current-head baseline.

## What the existing measurements establish

The [paired-row record](../../completed-work/performance.md#rows-of-pair-data-in-vectors)
contains matched Zen 4 measurements after issue #930. Its x86-64 n-body
`list[Body]` step changed from 219.2 cycles / 652 instructions to 185.5 /
494. The same session's fixed Rust `[Body; 5]` measured 157.3 / 451. That
comparison motivates exposing related components, but does not isolate type
syntax: Rust also knows body count statically, and its optimizer packs z and
mass. The current Plenty source guards a dynamic body count.

The same record shows the fixed-state fixture's own decaying values at v3
changing from 12.05 to 13.6 cycles with paired rows. The semantics are
unchanged; a packed dependency chain can wait for another subnormal lane.
Declaring a vector gives the user a choice and a documented cost, not an
assumption that wider arithmetic always wins.

Later [256-bit measurements](../../completed-work/performance.md#f64x4-in-ymm-registers-issue-962-stage-4)
show why n-body itself must be measured. At v3, the `f64x4` magnitude loop
with an `f64x2` tail changed from 200.5 cycles / 540 instructions using two
halves to 207.6 / 550 using `ymm`. Scalar-tail results moved in the opposite
direction, 216.0 / 539 to 213.7 / 523. Packed sqrt/div kernels improved, while
whole-step data movement remained decisive.

The [narrow-spill experiment](../../completed-work/performance.md#narrow-spills-beside-256-bit-values-issue-969-not-adopted)
measured a separate session: narrowing non-256-bit spills changed 185.4 to
185.0 cycles, and instructions 403.2 to 401.2, leaving most of the gap
unexplained. It was not adopted. Do not mix these absolute cycle counts with
the earlier session or infer that fixing spill width will make a declared
three-vector profitable.

Retained tools and source are in `experiments/nbody-structural/`,
`experiments/nbody-spills/` and `experiments/nbody-rsqrt/`. The
structural study (`experiments/nbody-structural/README.md`) distinguishes
hand-written diagnostic CLIF from safe public Plenty and records exact-state
checks. Its old public SIMD limitations are historical; recheck them at the
prototype's pinned revision.

## Preconditions

The B83 [inline-array prerequisite](../../design/language/03-types/arrays.md)
is delivered in local PR #1262, including literals, indexing, copy/move behavior,
checked size calculations and class/list layout. Compare the four candidates
against that actual implementation. A mere
`array` spelling or smaller allocation does not establish native SSA continuity.

This proposal approves a bounded prototype. Pin Plenty, Cranelift,
regalloc2, compiler flags, target features and measurement host. The main
source audit at `caf7c247` identifies:

| Source | Relevant boundary |
| --- | --- |
| `src/codegen/portable_simd.rs` | Closed trusted SIMD shapes; private one/two-part SSA and scalar fallback; constructors, projections and operations must stay native together. |
| `src/codegen/fixed_state.rs` | Existing automatic scalar-pair representation, to retain for plain code. |
| `src/record.rs`, `src/op.rs` | Slot/record width, field offsets, ownership and type facts; candidate compact payload cannot be assumed from register shape. |
| `src/frontend/clists.rs`, `plenty-runtime/src/aggregates.rs` | Primitive-column admission and storage/row operations; a vector field is an explicit new layout case. |
| `stdlib/std/simd.plenty`, `book/src/design/stdlib/signatures.md` | Existing public lane naming, borrowed inputs, ordered reduction and scalar behavior. |

Recheck this historical source audit against the prototype's pinned revision;
B83 has since shipped. Do not copy unmerged experiments into a feature or modify backend forks
without the documented fork procedure.

## Matched prototype experiment

Follow the [emitted-code procedure](../../development/emitted-code.md): build
an optimized compiler in the task worktree, isolate scalar/declared-vector/C
kernels, inspect assembly, attribute every extra instruction, and test one
CLIF shape per variant before changing lowering. Preserve all required checks.

Use small three/four-component add/subtract, scalar multiply, ordered dot,
length, field-update and list/column traversal kernels. Include component-only
reads and mutable component borrows to measure extraction/materialization
cost. Compare direct vector SSA, array-based scalar code, current `f64x4`
with explicit padding, the existing heuristic, and matched C. C's association,
FMA policy, bounds knowledge and alignment must match the Plenty case.

For n-body compare dynamic-list and fixed-array sources independently. Keep
the algorithm, arithmetic association, body-count knowledge and pair-update
order equal within each comparison. Rerun the current compiler and C/Rust
references in the same session, pinned to the same physical core. Count
`instructions:u` first, then interleave CPU/cycle runs over several function
layouts. Report medians, spread, host load, step counts, code size, stack size,
spills, register moves, lane shuffles and checks. Include compile-time counts
and representative unrelated programs. A new instruction reduction with a
repeatable whole-program slowdown does not pass the adoption gate.

Correctness covers full state as well as energy: normal/subnormal operands,
positive and negative zero, infinities, NaNs, empty lists, one element,
non-multiple storage lengths and varying steps/body counts. Compare observable
bits to the scalar reference except for the existing non-guarantee of NaN
payload identity. Ensure dot order with cancellation-heavy examples, and
verify all profiles/CPU levels produce identical promised results. Check
allocation errors and growth preserve collection contents. Negative tests
retain invalid indexing, unsafe borrow and overflow rejection.

Padding cases include infinite/NaN scalar multipliers, negation, poisoned
nonlogical storage, component writes, conversions, calls/returns, partial
loads and sum/class/list relocation. Inspect inactive operands before the
operation; masking a NaN afterward is not acceptance evidence. Explicitly
show that no uninitialized or out-of-range fourth word is read.

## Separate experiment review status

Local PR #1263 records a bounded native-SSA experiment and awaits review. Its
reported instruction reduction did not pass the whole-program CPU speedup
gate. This proposal merge adopts neither that experiment nor a public vector
API or compiler lowering. Its retained evidence remains in the separate PR.

## Acceptance and user guidance

Accept a final type only after results explain its benefit and its costs.
Otherwise retain the experiment and reject or revise the candidate. The
current heuristic and explicit SIMD remain useful outcomes if the new type
does not improve a meaningful source case.

Before handoff, perform self-review/fix cycles with focused tests until there
are no blockers; only then run the final full `just test` suite. The feature PR
also needs Clippy, formatting and complete docs CI. Update the std reference,
signature index, layout/collection references and a runnable tutorial lesson
alongside implementation, with a before/after n-body example grounded in the
new measurements. Teach when to use vectors, their alignment/padding and
component-access costs, and when ordinary fields are preferable. No tutorial
should present this candidate as shipped before that gate.
