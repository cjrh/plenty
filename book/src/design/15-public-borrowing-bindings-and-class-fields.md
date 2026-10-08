# Public borrowing — bindings and class fields

`&items[index]` and `&mut items[index]` borrow list elements or dictionary values.
List indices accept negative offsets; invalid indices/missing keys trap like
ordinary indexing. The collection must be a named owner, reference, or projected
place. Element loans conservatively cover the collection and block invalidating
growth, removal, replacement, or owner moves while live. Different indices are
not proven disjoint. Class fields and nested list/dictionary elements can be
projected through these references, and the existing single-origin return rule
also permits returning an element reference. No allocation occurs when borrowing.

[References and explicit copying](../proposals/references-and-copying.md) records
the accepted ownership decision. References to named bindings and borrowed function
parameters and their class fields are implemented as `&T` and `&mut T`. `str` remains the sole string
value type; `&mut str` permits replacement of a string binding, not byte mutation.

Reference bindings are immutable and initialized by a direct `&name` or
`&mut name`, including field paths; reassignment and implicit reference aliases are rejected initially.
Reborrowing an existing reference is supported. An exclusive parent may lend shared
or exclusive access, with conflicting parent access prohibited while the child is
live. Calls automatically reborrow reference arguments, using declared signatures
only. Shared references cannot be upgraded to exclusive ones.

The frontend records loan origins and reads/writes of whole binding places.
The independent checker computes backward loan liveness over explicit control-flow
edges to a fixed point, including backedges and early exits. A conflicting write,
move, drop, or exclusive borrow is rejected while a loan remains live. Shared
reads conflict with live exclusive loans. Parent origins are tracked through
reborrows; access through a parent conflicts with a live child. Copies of immutable
values finish their read immediately; observations of mutable collections hold
temporary shared loans through the operation that consumes them.

Native references address typed local storage slots, generator frame slots, or
fixed class field slots, or collection entry slots protected against invalidation.
The internal reference value contains a 64-bit address and a 64-bit inline-sum
tag offset. Reads decode the selected payload; writes preserve its enclosing
variant tags. These two words travel by value, without allocation or a temporary
reference descriptor. Ordinary references have a zero offset. C ABI adapters
continue to expose ordinary pointers and marshal them to the internal form.
Each slot starts with 128-bit value bits; slots containing a range have an adjacent
32-byte payload. Writes through references update that inline storage. Functions
taking addresses or storing inline ranges spill their locals; other functions retain SSA locals.
Borrowed parameters already carry an address. Internal retained operands protect
temporary storage lifetime, but the static checker establishes access permissions.
Reference calls retain the caller frame, so native tail calls do not invalidate it.

Partial moves and stored references remain rejected. Class field loans distinguish disjoint projections,
including through reborrowed reference parameters. A generator cannot capture reference parameters or retain a live
loan across `yield`; short borrows completed within one resume are permitted.
No lifetime annotation syntax or general trait system is required for this subset.

Tuples are immutable structural products. A tuple containing owned fields is
affine; whole-tuple unpacking transfers each component and `_` drops a component.
Copyable tuples share immutable storage on assignment. The initial implementation
uses the existing heap record representation and cleanup machinery. A tuple
display `(a, b)` returns
`Result[tuple[A, B], AllocError]` and consumes and cleans up evaluated components
on failure. Component expressions retain their own allocation/error contracts.
Indices must be nonnegative integer literals, checked against the tuple's arity;
owned fields can be observed or explicitly copied, but ownership extraction
requires unpacking. References and generators cannot be stored in tuples yet.
Empty `()` remains unit. Nested/starred unpacking is deferred.

Shared iteration over a list of owned elements binds `&T`; mutable list iteration
binds `&mut T`, including scalar elements. Shared iteration over copyable elements
still binds values. The source remains borrowed throughout the loop, including
back edges, so growth, removal, replacement, and owner destruction cannot
invalidate element addresses. Indexed and iterated element loans retain the
collection's entire footprint; disjoint indices are not proven. Loop variables
cannot escape into stored references. Returned element references follow the same
single reference-parameter origin rule as direct indexed borrows.

Prefer last-use/flow-sensitive loan checking over lexical-lifetime rules.
Polonius is the relevant Rust work: it models relationships between reference
origins and loans over control flow. Rust's Polonius alpha was enabled on
nightly in August 2026; stabilization and formal modeling remain work items.
The old standalone Datalog engine is not automatically the current rustc
implementation. We should reuse concepts and test cases, not assume that
adding a crate supplies a sound checker for Plenty.

The implementation supports local and restricted returned borrows without stored references.
Once projected source places,
aliasing, moves, reborrows, joins, and drop points are modeled, add a restricted
reference-return rule whose origin is unambiguous from the signature. Reject
ambiguous cases before introducing lifetime syntax. Never infer cross-function
borrowing contracts by inspecting callee bodies. A small sound subset is
preferable to a permissive checker with gaps.

Correctness gates include use-after-move, conflicting shared/exclusive loans,
mutation during a live shared borrow, branch-dependent loans, reborrows,
returning local references, partial moves, and ownership across control-flow
joins. Extend regression coverage and cleanup checks whenever the supported
reference subset grows; the current checker is not a complete Polonius implementation.

Sources informing this design:

- [Polonius alpha nightly announcement](https://blog.rust-lang.org/2026/08/04/enabling-polonius-alpha-on-nightly/)
- [2026 Polonius stabilization/modeling goal](https://goals.rust-lang.org/2026/polonius.html)
- [Borrow checker roadmap](https://goals.rust-lang.org/2026/roadmap-borrow-checker-within.html)
