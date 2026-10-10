# Borrowing and reborrowing

`&place` creates a shared reference `&T`; `&mut place` creates an exclusive
reference `&mut T`. Exclusive access requires a mutable owner or an existing
exclusive reference. A shared loan permits other shared reads; an exclusive
loan prohibits competing reads, writes, and borrows. Neither acquires ownership.
`&mut str` permits replacing a string binding, not mutating string bytes.

A reference binding can be initialized from an explicit borrow, a named
reference, or a reference-returning call. Naming or passing an existing
reference reborrows it. An exclusive parent can lend shared or exclusive access;
conflicting parent access is prohibited while the child is live. A shared
reference cannot be upgraded to exclusive access. Reference bindings are
immutable unless declared `mut`; that declaration controls reassignment of the
reference, independently of the referent's shared or exclusive access mode.

Loans are checked through branches, loops, and early exits and normally end at
their last use. Moving, replacing, or destroying an owner conflicts with any
live loan into it. References cannot be stored in aggregates or retained across
generator suspension. Partial moves and general lifetime annotations are unsupported.

Class fields and fixed tuple indices are projected places. Disjoint fields can
be borrowed independently; access to an ancestor or whole owner overlaps its
descendants. `&items[index]` and `&mut items[index]` borrow list elements or
dictionary values in named storage, including projected collections. Indices
and keys follow ordinary indexing, including negative list indices and traps
for absent entries. Element loans protect the whole collection: different
indices are not proven disjoint, and invalidating mutation or movement conflicts.
Nested fields and collection elements can be projected through these references.
Borrowing itself allocates nothing.

An ordinary `&T` parameter implicitly borrows an owned argument: `inspect(value)`
and `inspect(&value)` lend the same owner. An `&mut T` parameter requires
`&mut value`, except when forwarding an existing exclusive reference. Methods
borrow their receivers according to their declared `self` parameter. Generic
calls and callbacks follow the same signature-based rules.

Shared call arguments may be literals or owned temporaries. They evaluate once
in argument order and stay alive through later arguments and the containing
full expression, including cleanup on `?`. A reference returned into such a
temporary can be used within that expression, but cannot be bound, returned,
or retained by a scoped worker. Mutable temporary arguments and general
temporary lifetime extension are unsupported.
