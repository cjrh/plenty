# Stored concrete closures

A generic class field can own a reusable closure with wholly owned captures.
Constructor inference preserves the producer identity: `Holder(make(1))` and
`Holder(make(2))` have the same concrete type when `make` returns the same closure
expression. Different closure expressions remain distinct types, even if their
call signatures agree. An unresolved `Closure[...]` annotation alone does not
specify a field's storage layout.

The environment occupies the class's field storage directly. Class construction
is fallible as usual; storing its callback adds no separate allocation. Calling
`owner.callback(...)` borrows the field, exclusively when the closure has mutable
captures. Such a call requires a mutable owner. Moving, replacing, and dropping
the containing owner preserves the environment's ordinary affine semantics.
Failure during class construction releases transferred captures exactly once.

`copy` and equality are unavailable for values containing environments. Printing
a closure displays `<closure>` without traversing captures. Shared immutable
strings inside captures retain their ordinary ownership semantics.

Tuples can own environments in their separate typed slots. Different slots may
have different producer types. A constant index borrows a callback for invocation;
mutable callbacks require a mutable, affine tuple owner. Disjoint tuple slots can
be borrowed separately. Unpacking transfers the environments into local owners.
Tuple construction retains its ordinary fallible allocation contract.

Lists can store owned reusable environments from one concrete producer. Literals,
comprehensions, inferred generic `list[F]` signatures, append/extend, reversal,
borrowed iteration, consuming iteration, and `pop` preserve inline environments.
Index calls borrow the collection; a live callback loan prevents resizing or
removal. Extraction, invocation, and destruction allocate nothing. Appending may
allocate when capacity is exhausted. Pop followed by append can reuse that slot.

Borrowed captures, unresolved layouts, and generator-containing environments
cannot enter heap storage. Consuming environments, dictionary values,
and user enum payloads retain their existing storage restrictions in this subset.
These rules also apply through inline `Option` and `Result` wrappers.

See the [representation decision](../proposals/stored-closures.md) for the costs
of heterogeneous boxing and bounded erasure, and the
[runnable lesson](../tutorial/80-store-owned-closures.md) for a concrete record.
