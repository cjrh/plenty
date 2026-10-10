# Recursive owned data

> **Status: Partly implemented.** Recursive classes, enums, aliases, and finite
> generic instances are implemented, with moves, matching, borrowing, and
> allocation-free drop. [Recursive storage](../design/language/03-types/recursive-data.md)
> and [boxes](../design/facilities/01-builtins/boxes.md) own the contract.
>
> **Shipped differently:** this page first proposed that classes and enums stay
> heap records, so recursion needed no `Box`. Classes and enums are now inline
> values, and every storage cycle must pass through `Box[T]` or a collection.
> The source model below describes what shipped.
>
> **Remaining:** deep copy, equality, and formatting of recursive values (B33);
> stored references (B11). Shared mutable graphs, weak edges, and a garbage
> collector are out of scope.

## Source model and finite storage

Classes, tuples, and enums store their fields inline, so a type that contained
itself directly would have infinite size. A recursive field goes through a heap
boundary: `Box[T]`, a list, a set, or a dictionary. A cycle without one is
rejected, and the diagnostic names the boundaries to use.

```plenty
class Node:
    value: i64
    next: Option[Box[Node]]

enum Tree:
    Leaf(i64)
    Branch(Box[Tree], Box[Tree])
```

`Node(1, Nothing)` and `Tree.Leaf(1)` allocate nothing and cannot fail. `Some(node)`
is an inline ownership wrapper. `Box(value)?` is the one allocation per link, and
its failure drops the value it consumed. Lists of children use their existing
fallible allocation API.

Keep transparent alias cycles invalid: `type Loop = Option[Loop]` has no nominal
ownership boundary and must not be expanded forever. Separately distinguish a
finite nominal recursion from infinitely growing generic instantiations such as
`Node[T]` containing `Node[list[T]]`. Neither a type spelling's nesting limit nor
the runtime value's depth is a substitute for that distinction.

## Ownership and destruction

Recursive classes and enums move under the existing rules. Aggregates containing
recursive values are also affine, and automatic deep copying is rejected. Mutable
collections or classes already make their containing values affine. Storing an
owner inside its own descendant must fail the normal move/borrow checks. There
are no stored references or weak edges in this design, so source ownership cycles
are not introduced by recursive *types*.

Drop uses the existing intrusive destruction queue and must allocate nothing,
even for a very deep chain. A class hook still runs before its fields, and explicit
nested drops in a hook finish synchronously. Queue links reuse dead header storage
only after atomic final-release ownership has been established. Recursive values
do not become transferable across threads merely because their counts are atomic;
the [worker eligibility rules](../design/facilities/03-concurrency/index.md) apply
to every reachable field and destructor effect.

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

Construction, movement, matching, field borrowing, and iterative drop are
implemented. Deep `copy`, structural equality, hashing, and formatting recurse
through runtime values, so on recursive data they would turn ordinary valid
values into stack overflows. The compiler rejects them for any type whose stored
values can recur.

B33 lifts that restriction. Prefer iterative fallible work storage for
already-fallible copy and formatting. Equality and other allocation-free
operations cannot silently add a fallible allocation; they need a bounded-memory
traversal. Direct user-written iterative traversals remain possible. This
restriction is preferable to a hidden small maximum on the depth of every
recursive value.

The [backlog](../backlog.md) tracks implementation and acceptance work; this page
records the representation and behavioral constraints, not a second task queue.
