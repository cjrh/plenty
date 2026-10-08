# Recursive data

Classes and user enums can refer to themselves or one another, including through
`Option`, `Result`, tuples, lists, dictionary values, and transparent aliases.
Concrete generic instances can recur too: `Node[T]` may contain
`Option[Node[T]]`. Finite mutual instances such as `Link[A, B]` and `Link[B, A]`
share their usual specialization cache.

Existing heap records provide indirection. Constructing a class or user enum
allocates its one record and returns `Result[..., AllocError]`; no additional
`Box` type or allocation is necessary. Standard sums stay inline. Type recursion
does not place a fixed limit on the number of nodes in a runtime value.

Recursive values and aggregates containing them move on assignment and owned
argument passing. Mutable fields retain ordinary explicit borrowing rules.
`match` consumes recursive enums and transfers bound payloads; an iterative
`while`/`match` loop can dismantle a chain without recursive function calls.
Matching references, partial moves from classes, stored references, shared mutable
graphs, and source ownership cycles remain unsupported. A terminating variant,
`Nothing`, or an empty collection supplies the usual construction base case.

Automatic `copy`, structural equality (including list membership), `print`, and
`str.repr` reject types whose stored values can recur. This also applies through
standard sums and containers, even if a particular value is shallow. Their runtime
traversals do not yet support arbitrary depth. Printing scalar fields, comparing
selected fields, and writing explicit traversal functions remain available.
Dictionary keys and set elements retain their existing closed set of hashable
types. Callable signatures do not constitute stored recursive values.

Automatic drop uses the existing allocation-free intrusive queue. Class hooks
run before fields; ordinary scope exit, replacement, early return, and failed
construction retain their cleanup rules. Native tests build class and enum chains
of 100,000 nodes each, then drop both with allocation disabled on a 256 KiB stack.
Explicit recursion in user methods or destructor hooks still uses the native stack.

The compiler reserves nominal identities before resolving fields. A table owns
immutable definitions; stored edges use weak handles and escaped type handles
retain the table. Identity uses the canonical declaration name and concrete type
arguments, never expanded fields. Successful and failed compilation release their
type tables without reference-count cycles.

Aliases resolve with an iterative dependency walk. Definitions and native metadata
use work queues. Type facts inspect finite graphs with visited identities; leaf
elimination identifies cycles and owners that reach them. Copyability, destruction,
affinity, and float-sensitive equality metadata are computed from reachable storage.
Only complete facts are cached. Heap nominal boundaries cut layout cycles while
surrounding inline sum tags still count toward the nesting limit.

Alias-only cycles remain invalid and receive bounded dependency-path diagnostics.
Specialization remains bounded at 256 generic instances, 64 expansion/nesting
levels, and 16,384 bytes per concrete type name. An endlessly expanding instance
such as `Node[T]` containing `Node[list[T]]` is rejected when specialized. These
compiler limits concern type descriptions, not runtime chain or tree depth.
