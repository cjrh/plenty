# Borrowed enum matching

`match &value` inspects an enum without consuming it. Matching an existing `&E`
reference has the same behavior. Each named payload binding has type `&T`, even
for a scalar or unit payload; `_` creates no binding. Ordinary owned matching
retains its previous transfer/copy behavior. Patterns, coverage, and arm scoping
are unchanged. The scrutinee evaluates once.

`match &mut value`, or matching an existing `&mut E`, binds payloads as `&mut T`.
This supports inline standard sums and affine user enums, including recursive
enums and enums containing owned collections/classes. Payloads of copyable
heap-backed enums remain immutable because assignments may share their storage.
Mutable matching of those enums is rejected: use a shared match and replace the
whole enum instead. Inline sums containing such enums may still be matched
mutably to replace the stored enum handle.

Borrowing and matching allocate nothing. Heap payload references address the
original record slots. Inline `Option`/`Result` and builtin error payloads use
the [internal reference tag offset](15-public-borrowing-bindings-and-class-fields.md)
to refer into the original slot. Nested references travel through calls and
closure captures by value. Reads decode the selected tags; writes retain every
enclosing variant tag. References to inline range or generator payloads also
preserve the original adjacent storage. No reference points at an unpacked
temporary.

Each payload loan descends from the scrutinee's loan. It protects the owner and
active variant from conflicting access, replacement, movement, and destruction
until its last use. Disjoint payload fields can be borrowed mutably together.
Class field projections remain precise; collection origins and returns without
a fixed field summary keep their conservative footprint. Matching a variant
reads its discriminant and therefore conflicts with an outstanding mutable loan
into that enum.

Bindings exist only in their arm. Last-use checking permits replacing the owner
after the final payload use, even within the same arm. Branches, loops, early
returns, `?`, `break`, and `continue` use the ordinary access CFG. No live loan
may cross `yield`. Wildcards do not consume or move borrowed payloads.

Payload references can be returned under the existing
[single reference-parameter origin rule](18-returned-references.md). Every
return path must originate in that parameter. Borrowing a temporary, returning a
reference into a local owner or an owned match binding, and storing references
in aggregates remain rejected. A returned payload conservatively protects the
caller's entire borrowed argument.

A `mut` reference binding can be assigned a payload of a match on itself, so a
loop follows a chain without recursive calls. Payload bindings themselves are
immutable. See
[reference bindings](15-public-borrowing-bindings-and-class-fields.md).

Native tests cover recursive class and enum traversal, imported generic classes,
shared/mutable returns, disjoint fields, nested inline replacement, ranges,
closure captures, collection entries, cleanup, and all 64 packed tag positions.
Runtime allocation checks verify traversal and mutation with allocation disabled.
The [runnable lesson](../tutorial/78-match-borrowed-values.md) teaches these rules.
