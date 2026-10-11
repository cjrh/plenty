# Recursive storage

Classes and user enums may refer to themselves or one another, including through
`Option`, `Result`, tuples, and transparent aliases. Every storage cycle must pass
through a heap boundary: `Box[T]`, a list, a set, or a dictionary. A cycle without
one would require an infinite-sized value and is rejected.

```python
class Node[T]:
    value: T
    next: Option[Box[Node[T]]]
```

Finite concrete generic instances can recur, including mutual instances with
different arguments. Alias-only cycles remain invalid. Endless type expansion,
such as a `Node[T]` containing a `Node[list[T]]`, is rejected when specialized;
the diagnostic names the first three instances and points at the type
application that grows. Finite nesting deeper than 64 levels reports that limit.
These limits concern type descriptions rather than the number of nodes in a
runtime chain or tree.

Recursive values and aggregates containing them move. Automatic `copy`, equality
(including membership), and formatting reject types whose stored values can recur,
even when a particular value is shallow. Callable signatures do not constitute
stored recursive values. Consuming matching transfers payloads;
borrowed matching preserves the owner. See [ownership](../05-ownership/index.md).

Partial moves from classes, stored references, shared mutable graphs, and source
ownership cycles are unsupported. A terminating variant, `Nothing`, or an empty
collection supplies a construction base case.

Automatic destruction of recursive owners uses bounded stack space and allocates
nothing. Explicit recursion in user methods and destructor hooks still uses the
native stack. `Box` construction, content conversion, projections, and borrowing
are described under [facilities](../../facilities/index.md).
