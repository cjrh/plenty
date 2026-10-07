# Allocation-free generator frames and ranges

> Generator design under discussion, not implemented behavior. Ranges are now
> allocation-free independently of generators. Active work and ordering
> live in the [backlog](../backlog.md). The
> [generator reference](../design/16-native-generators.md) describes today's ABI
> and source contract.

## Objective and boundary

Creating a generator should require no heap allocation for its execution frame.
The compiler already emits a resumable native state machine; replacing that
lowering is unnecessary. The change is how its state is represented, owned,
passed, and returned.

This guarantee does not cover allocations performed while evaluating arguments
or running the body. A generator may own a list or file, yield an allocated
value, or execute fallible operations. Those operations retain their existing
error contracts. Collecting a generator into a list still returns `Result`.

Proposed construction returns the generator value directly, without an
`AllocError` wrapper. Body execution remains lazy. `next` returns `Option[T]`,
and exhaustion, consuming iteration, moves, and early cleanup retain their
current semantics. This is a representation guarantee across supported uses,
not an optional optimization limited to immediately consumed generators.

## Concrete types and source annotations

Today's `Generator[T]` identifies only the yielded type. A pointer to a heap
frame lets unrelated bodies, with different frame sizes, use that one type.
Inline storage needs to preserve more information.

The recommended design gives each generator body and concrete specialization
its own compiler-inferred type and finite layout. Users would still declare
`-> Generator[T]` on a yielding function; this describes its yield contract,
while the function identity determines the concrete returned frame. No user
needs to spell a generated type name.

This choice is deferred with the generator work. The alternative is keeping a uniform,
size-erased owned type with fallible allocation; that cannot provide the same
general inline-storage guarantee.

Factories, annotated parameters, and standard sum wrappers need an explicit
contract before implementation:

- A factory returning `Generator[T]` should resolve to one concrete frame shape
  for each specialization. Returning different shapes on different branches
  requires a finite explicit sum or a future boxed form, with no hidden boxing.
- Parameters written as `Generator[T]` could introduce a hidden concrete type
  parameter, specializing consumers by frame identity. Alternatively, require
  an explicit generic iterator constraint. Choose one rule consistently for
  values, references, aliases, and nested `Option`/`Result` annotations.
- `Option` and `Result` containing a generator must preserve its concrete layout.
  The existing scalar-sized wrapper representation cannot simply retain a
  pointer to a callee's expired stack frame.
- Distinct generator bodies are distinct types even when they yield the same
  element type. Assignment, branch joins, and return diagnostics must explain
  this without exposing compiler-generated names as required source syntax.

## Storage, moves, and destruction

Use caller-owned result storage for returned frames and a compiler-computed
size/alignment for each concrete layout. Frames live inline in their owning
locals, arguments, wrappers, or enclosing generator frames. The initial scope
need not lift existing restrictions on generators in user collection/class
storage.

Nested frame layouts form a dependency graph. Detect recursive inline layouts
and explain the need for indirection; do not reserve a global maximum frame size
or quietly allocate. Cache layout and specialization work to keep compilation
predictable.

Moves transfer ownership exactly once. A moved frame cannot retain absolute
pointers into its previous storage. Continue rejecting references across
suspension; compute accesses relative to the active frame. Revisit tail-call
eligibility whenever an argument points into storage that the tail call removes.

Keep initialized-state information sufficient to drop captures and suspended
locals on exhaustion, `break`, return, propagation, or explicit `drop`. Destroy
inline nested frames without freeing their storage as heap objects. Resuming
after a move must use the new owner, and repeated exhaustion must stay stable.

## How ranges fit

An allocation-free generator can implement range traversal, but a generator
itself is single-use and has no length, indexing, or membership operations.
Preserve these existing range capabilities with a small inline `range[T]` value
holding its bounds and signed step. Each traversal creates a concrete generator
from copied scalar fields, with no captured reference to the range.

Range construction and the existing indexed traversal are allocation-free
without generator changes. Sharing a future generator implementation
is an optional follow-up, not a prerequisite.

Using such a generator would require an iteration dispatch rule shared by `for`, comprehensions, and
collection constructors. A public iterator protocol is a separate API decision;
do not require one merely to remove the existing builtin allocations.

Typed range traversal must preserve unsigned bounds, descending signed steps,
termination near integer limits, and once-only argument evaluation. A naive
`current += step` generator is insufficient: even the final unused increment
can overflow. Source-level implementation may need checked arithmetic helpers.
Zero-step and length-limit policy are separate from allocation failure; today's
runtime rejects both. Do not silently change those rules with this refactor.

## Evidence required

Disable heap allocation and exercise scalar-only generators through creation,
resume, moves, factories, consumers, nested generators, and `Option`/`Result`.
Verify cleanup exactly once before first resume, after suspension, on exhaustion,
and on all early exits. Include compile-fail cases for incompatible frame types
and recursive layouts, plus native memory checks for stack lifetime errors.

For ranges, cover repeated and nested traversal, indexing/membership/length,
all integer widths, both step signs, maximum bounds, empty ranges, and unchanged
invalid-input diagnostics. Check frame sizes and compilation cost as well as
runtime allocation counts. Update the reference and runnable lessons only when
the corresponding behavior is implemented.
