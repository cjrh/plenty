# Recursive owned data

The initial source contract is implemented; see the [recursive-data reference](../design/30-recursive-data.md)
for its current limits. This design record explains the ownership, fallible
construction, and allocation-free `Option`/`Result` choices. It does not add
shared mutable graphs or a garbage collector.

## Source model and finite storage

Classes and user enums already own heap records. A reference to such a value
occupies a fixed-size ownership slot, so these existing representations can break
a recursive layout cycle. A second mandatory `Box` allocation would add cost and
another concept without solving a current layout problem.

For example, the intended form is:

```text
class Node:
    value: i64
    next: Option[Node]

enum Tree:
    Leaf(i64)
    Branch(Tree, Tree)
```

`Node(1, Nothing)?` would allocate one class record. `Some(node)` would remain an
inline ownership wrapper. A `Tree.Branch(left, right)?` would allocate one enum
record and transfer its children. If allocating the parent fails, its consumed
arguments receive the normal constructor cleanup. Lists of children use their
existing fallible allocation API. No new infallible construction path is needed.

Keep transparent alias cycles invalid: `type Loop = Option[Loop]` has no nominal
ownership boundary and must not be expanded forever. Separately distinguish a
finite nominal recursion from infinitely growing generic instantiations such as
`Node[T]` containing `Node[list[T]]`. Neither a type spelling's nesting limit nor
the runtime value's depth is a substitute for that distinction.

## Ownership and destruction

Recursive classes and enums move under the existing rules. The implemented subset
also makes aggregates containing recursive values affine and gates automatic deep
copying. Mutable collections or classes already make their containing values affine. Storing an
owner inside its own descendant must fail the normal move/borrow checks. There
are no stored references or weak edges in this design, so source ownership cycles
are not introduced by recursive *types*.

Drop uses the existing intrusive destruction queue and must allocate nothing,
even for a very deep chain. A class hook still runs before its fields, and explicit
nested drops in a hook finish synchronously. Queue links reuse dead header storage
only after atomic final-release ownership has been established. Recursive values
do not become transferable across threads merely because their counts are atomic;
the [concurrency contract](native-concurrency.md) applies to every reachable field
and destructor effect.

## Compiler identity and bounded analysis

Use a compilation-owned table of nominal definitions. Type edges refer to stable
identities; the table owns definitions, avoiding an `Rc` cycle that leaks compiler memory.
Reserve identities before resolving fields, then finalize immutable definitions.
Aliases resolve to those identities. Type equality and hashing compare nominal
identity and concrete generic arguments, never recursively expanded fields.

Compute properties over the finite type graph, with explicit monotone rules for
copyability and destructor presence; a temporary placeholder must not cache a
wrong fact. The implementation uses visited identities and leaf elimination to
find recursive storage and aggregates that reach it. More detailed component
analysis may support future thread effects without changing source syntax.
Keep finite inline-layout checks distinct from heap graph recursion. Emit native
descriptor IDs before their edges, as the current metadata emitter already does.
Every metadata/visibility/debug traversal needs visited identities or a bounded
worklist. Declaration errors should name the actual dependency cycle.

## Operations that need their own depth contract

Construction, movement, matching, field borrowing, and iterative drop are the
first useful subset. Deep `copy`, structural equality, hashing, and formatting
currently recurse through runtime values; the bounded acyclic source types have
limited that depth until now. Removing the declaration restriction without
addressing these operations would turn ordinary valid values into stack overflows.

Prefer iterative fallible work storage for already-fallible copy and formatting.
Equality and other allocation-free operations cannot silently add a fallible
allocation. Until a bounded-memory traversal is selected, diagnose those operations
on recursive components instead of claiming general support. Direct user-written
iterative traversals remain possible. This restriction is preferable to a hidden
small maximum on the depth of every recursive value.

The [backlog](../backlog.md) tracks implementation and acceptance work; this page
records the representation and behavioral constraints, not a second task queue.
