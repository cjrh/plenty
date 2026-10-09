# Stored concrete closures

A generic class field can own a closure with wholly owned captures.
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

Lists can store owned environments from one concrete producer. Literals,
comprehensions, inferred generic `list[F]` signatures, append/extend, reversal,
borrowed iteration, consuming iteration, and `pop` preserve inline environments.
Index calls borrow the collection; a live callback loan prevents resizing or
removal. Extraction, invocation, and destruction allocate nothing. Appending may
allocate when capacity is exhausted. Pop followed by append can reuse that slot.

Dictionary values follow the same concrete storage rules. Indexing borrows a
callback, replacement drops the old environment, and `pop` transfers a value into
`Option` without allocation. Growth and update relocate environments with their
typed rows. Keys remain restricted to the ordinary hashable scalar types; closures
cannot be keys or set elements. Consuming item iteration transfers value owners.

Generic user enum payloads can hold these concrete environments too. Borrowed
matching lends a callback in place; consuming matching transfers it to the arm.
An ordinary enum can enumerate a finite family of different callback shapes,
without erasing their types. Enum construction is fallible as usual.

Consuming environments use the same storage. Invoke them only after extracting an
owner with `pop`, consuming iteration/matching, or tuple unpacking. Calling a
one-shot callback through an indexed or borrowed field is rejected. Abandoned jobs
release captures; called jobs transfer captures into the body. A class field can
own and drop a one-shot callback, but ordinary class fields cannot be moved out.

Borrowed captures, unresolved layouts, and generator-containing environments
cannot enter heap storage in this subset.
These rules also apply through inline `Option` and `Result` wrappers.

[Scoped workers](32-scoped-native-threads.md) can borrow stored callbacks and
transfer owned concrete callback results at join. Eligibility inspects both the
environment's captures and its body, including hidden destructor effects.

See the [representation decision](../proposals/stored-closures.md) for the costs
of heterogeneous boxing and bounded erasure, and the
[runnable lesson](../tutorial/80-store-owned-closures.md) for a concrete record.
