# Borrowed matching

`match &value`, or matching an existing `&E`, inspects an enum without consuming
it. Named payload bindings have type `&T`, even for scalar or unit payloads.
`match &mut value`, or matching an existing `&mut E`, binds `&mut T` payloads.
`_` creates no binding and moves no borrowed payload. The scrutinee evaluates
once; patterns, coverage, and arm scope follow ordinary
[matching](../04-expressions/index.md). A binding inside a nested pattern is a
reference to that inner payload, through boxes as well, and a name bound by
`p | q` refers to the payload of whichever alternative matched.

Mutable matching supports standard sums and user enums, both copyable and affine.
Mutating a copyable enum's inline payload leaves independent copies unchanged.
Matching and its payload borrows allocate nothing.

Payload loans descend from the scrutinee loan and protect the active variant
against conflicting access, replacement, movement, or destruction. A nested
binding's loan also protects each enclosing variant. Disjoint
payload fields can be borrowed mutably together; precise class-field origins
remain precise, while collection and imprecise returned origins protect their
whole target. Reading a discriminant conflicts with an existing exclusive loan
into the enum.

Bindings exist only in their arm. Last-use checking permits replacement after
the final payload use, even within that arm. No live payload loan may cross
`yield`. Payload references can be returned under the
[single reference-parameter rule](04-returned-references.md); the returned
payload conservatively protects the caller's whole argument. Borrowing a
temporary, returning a local owner's payload, and storing payload references
in aggregates are rejected. A `mut` reference binding can be reassigned to a
payload borrowed from itself; the arm's payload bindings are immutable.
