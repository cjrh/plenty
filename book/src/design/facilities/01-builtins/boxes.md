# Boxes

`Box(value) -> Result[Box[T], AllocError]` moves a value into one heap allocation.
Failure drops the input. Boxes are affine. Moving, borrowing, extracting content,
and destroying the box add no allocation; explicit copying and operations on its
content retain their own costs. Boxes provide a heap boundary for
[recursive data](../../language/03-types/recursive-data.md).

For insertion into a borrowed structure, allocate a detached node before moving
existing contents into it. Otherwise a failed allocation can destroy the old
contents as part of its input. The [insertion lesson](../../../tutorial/09-generics/recursive-insertion.md)
demonstrates preservation without adding a successful-path allocation.

Where a T is required, `Box[T]` converts to T by moving out the content and
freeing the box, without failure. This applies to arguments, returns, annotated
bindings, assignments, constructor fields, and collection elements. Borrowed
boxes convert to references to their contents. Nested boxes convert through
every level.

Without an expected content type, the box remains a box: `other = box` moves its
owner. Operators and conditions do not convert it. `*box` explicitly extracts
the content; `&*box` and `&mut *box` borrow it. Matching an owned box consumes it;
matching a borrowed box lends its content.

Class fields and methods are reached directly through boxes. Builtin list,
dictionary, set, and string methods also reach through nested boxes. Calling a
method on a boxed binding, field, or element borrows its content and preserves
the box. Mutation requires a mutable owner or exclusive reference.

Read-only methods accept owned temporary boxes, keeping content alive through
the expression. Mutation needs a binding, field, or element rather than a
temporary receiver. Boxed projections from temporary collections/classes and
temporary returned references require a named binding before builtin method calls.

Automatic copy, equality (including membership), print, and str.repr reject types
whose stored values can recur, including through containers or standard sums.
Use explicit traversal to inspect such values. Automatic box cleanup allocates
nothing and does not recurse on the native stack; user methods and destructor
hooks retain ordinary recursion and effect rules.
