# Recursive data

See the [runnable lesson](../tutorial/09-generics/recursive-data.md) for a consuming
chain traversal and a tree declared in an imported module.

Classes and user enums can refer to themselves or one another, including through
`Option`, `Result`, tuples, and transparent aliases. Every such cycle must pass
through a heap boundary: `Box[T]`, a list, a set, or a dictionary. Values are
otherwise stored inline, so a cycle without one would describe a value of
infinite size; the declaration is rejected, naming the type. Concrete generic
instances can recur too: `Node[T]` may contain `Option[Box[Node[T]]]`. Finite
mutual instances such as `Link[A, B]` and `Link[B, A]` share their usual
specialization cache.

`Box(value)` moves a value into one heap allocation and returns
`Result[Box[T], AllocError]`; failure drops the value. Boxing is the only box
operation that allocates, so it is the only one that needs syntax at the point
of use.

A `Box[T]` converts to `T` wherever a `T` is required: a function or method
argument, a return value, an annotated binding, an assignment to an existing
binding, field, or element, a constructor field, and a collection element. The
conversion moves the content out and frees the box; it cannot fail. A `&Box[T]`
converts to `&T` the same way. Nested boxes convert through every level. A boxed
class's fields and methods are reached directly, and matching a box matches its
content: an owned box is consumed, a borrowed one lends its content.

Builtin list, dictionary, set, and string methods also reach through nested
boxes. Calling a method on a boxed binding, field, or element borrows its content
and preserves the box. Mutation requires a mutable owner or an exclusive
reference; a shared reference never permits mutation. Read-only methods also
accept owned temporary boxes, which release the box and keep its content alive
through the full expression. Mutation still requires a binding, field, or
element rather than a temporary receiver. Boxed projections from temporary
collections or class instances, and temporary references returned by calls,
still require a named binding before calling these builtin methods.

Where no type is required the box stays a box: `other = b` moves the box.
Operators and conditions do not convert either. `*b` moves the content out
explicitly in those places, and `&*b` and `&mut *b` borrow it. Boxes are affine.
Type recursion does not place a fixed limit on the number of nodes in a runtime
value.

Recursive values and aggregates containing them move on assignment and owned
argument passing. Mutable fields retain ordinary explicit borrowing rules.
`match` consumes recursive enums and transfers bound payloads; an iterative
`while`/`match` loop can dismantle a chain without recursive function calls.
Matching a [shared or mutable reference](31-borrowed-enum-matching.md) instead
borrows the payloads and preserves the owner; a `mut` reference binding advanced
in a loop walks a chain of any length without recursive calls. For classes,
`replace(node.next, Nothing)` transfers an optional child out while leaving its
field initialized. Installing another owned tail instead supports iterative
relinking without cloning or allocating. The node must be mutable or exclusively
borrowed. Partial moves from classes, stored
references, shared mutable graphs, and source ownership cycles remain unsupported. A terminating variant,
`Nothing`, or an empty collection supplies the usual construction base case.

Automatic `copy`, structural equality (including list membership), `print`, and
`str.repr` reject types whose stored values can recur. This also applies through
standard sums and containers, even if a particular value is shallow. Their runtime
traversals do not yet support arbitrary depth. Printing scalar fields, comparing
selected fields, and writing explicit traversal functions remain available.
Dictionary keys and set elements retain their existing closed set of hashable
types. Callable signatures do not constitute stored recursive values.

Automatic drop releases boxes through the existing allocation-free intrusive
queue, so dropping a chain does not recurse. Class hooks run before fields;
ordinary scope exit, replacement, early return, and failed construction retain
their cleanup rules. Native tests build boxed class and enum chains of 100,000
nodes each, then drop both with allocation disabled on a 256 KiB stack.
Explicit recursion in user methods or destructor hooks still uses the native stack.
Exhausting it ends the program with a [stack overflow report](09-rust-runtime-packaging.md#stack-overflow).

Imports preserve the nominal identity and member visibility of recursive types.
Recursive classes can also remain behind existing opaque C export handles. Static
and shared library tests call their factories and shared borrows from C and from
generated Plenty wrappers, then verify matching destruction of the owned chain.
The recursive field layout is not exposed as a C record layout.

The compiler reserves nominal identities before resolving fields. A table owns
immutable definitions; stored edges use weak handles and escaped type handles
retain the table. Identity uses the canonical declaration name and concrete type
arguments, never expanded fields. Successful and failed compilation release their
type tables without reference-count cycles.

Aliases resolve with an iterative dependency walk. Definitions and native metadata
use work queues. Type facts inspect finite graphs with visited identities; leaf
elimination identifies cycles and owners that reach them. Copyability, destruction,
affinity, and float-sensitive equality metadata are computed from reachable storage.
Only complete facts are cached. Boxes and collections cut layout cycles, and a
cycle they do not cut is the infinite-size diagnostic. Surrounding inline sum
tags still count toward the nesting limit, and so does each box.

Alias-only cycles remain invalid and receive bounded dependency-path diagnostics.
Specialization remains bounded at 256 generic instances, 64 expansion/nesting
levels, and 16,384 bytes per concrete type name. An endlessly expanding instance
such as `Node[T]` containing `Node[list[T]]` is rejected when specialized. These
compiler limits concern type descriptions, not runtime chain or tree depth.
