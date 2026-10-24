# Columnar-list ownership and API boundary

This records the original proposed contract for [columnar lists](../columnar-list.md).
The [implemented subset](../../design/facilities/01-builtins/clists.md) uses
scalar-valued column pipelines, does not expose packed field references, and
does not yet provide removal. The alternatives below remain design rationale.

## An honest object facade

`items[i].field` can lower to a field-column address at row `i`, evaluating the
index once and preserving normal bounds diagnostics. It need not create an
object or a proxy allocation. A full `&items[i]`, however, cannot be an ordinary
`&T`: there is no contiguous `T` at that location. This is an ABI and lifetime
fact, not something a clever overload can hide.

For the initial version, support direct scalar field reads/writes and explicit
`copy_row(i) -> Option[T]`, which reconstructs an owned class from scalar fields
without allocation. This is a deliberate duplication operation even though
classes themselves remain affine. An out-of-range index returns `Nothing`.
The copied class can be passed to ordinary methods; changes to it do not write
back. Do not implicitly gather/scatter around a method call: methods may return
borrows, invoke callbacks or exit early, and hidden writeback changes observable
semantics. Plain `items[i]`, ordinary whole-row borrows, and direct row-method
calls should give a diagnostic pointing to field access or `copy_row`.

A later local row binding could allow `for row in &items: row.mass`, using an
ephemeral compiler-known projected place, not a stored `T` or `&T`. That would
need its own call, reborrow, escape and generator-suspension rules. Existing
local pipelines show that restricted compiler-known values are possible, but
do not implement this row representation. Treat general row iteration and
method adaptation as separate work rather than presenting them as free syntax.

## Candidate minimal operations

| Operation | Meaning |
| --- | --- |
| `clist[T]()` | Empty owner; no column allocation until needed |
| `len(items)` | Live row count, identical for every column |
| `items.reserve(additional)` | Fallible capacity for that many additional rows; no logical change on failure |
| `items.append(value)` | Move one complete `T`; return `Result[(), AppendError[T]]`, retaining the intact submitted row on failure |
| `items[i].field` | Checked scalar access, with ordinary field visibility; negative indices follow list conventions |
| `items[i].field = value` | Replace one field; keep the index and RHS evaluation order consistent with existing assignment |
| `items.copy_row(i)` | Allocation-free `Option[T]` for the initial scalar-only class subset |
| `items.remove(i)` | `Option[T]`; materialize and transfer the removed row, shift all columns together, preserve order |
| `items.column(T.field)` | Borrow a typed immutable sequence of that field's values, without allocating |

`T.field` in the last row would be a new compile-time field selector, not a
runtime string lookup or arbitrary class-variable expression. It must carry
the owning class and field type and respect visibility at the use site.
The compiler already knows class fields, but it does not expose selectors or
generic reflection. This proposal requires frontend support rather than a
magic generic library implementation. Concrete generic class instances may be
eligible after checking all instantiated field types; no runtime schema changes.

For example, this *proposed* pipeline uses scalar field references, not a
fabricated class reference:

```text
def add(total: f64, mass: &f64) -> f64:
    total + *mass

total = iter(particles.column(Particle.mass)).fold(0.0, add)
```

Extend existing `Iter` source lowering for a read-only column view. Keep it
local, borrowing the owner until consumption; do not require a general stored
reference feature first. Two immutable column views can feed `zip`; they have
the same length and row order. No public raw slice pointer or alignment promise
is necessary. If chunks arrive later, a column view remains a logical sequence,
not a promise of one allocation spanning all rows.

## Loans and mutation

Initially, any live projection or field reference protects the entire clist
against structural changes: append, reserve, removal, clear, movement and drop.
Do not advertise stable addresses merely because a chunk currently has room.
Scalar field references may use genuine `&Field`/`&mut Field` because that
individual field has contiguous storage. Begin with conservative owner-wide
conflicts; field- or chunk-disjoint mutable views require a separate proof.
Even two integer indices do not create simultaneous mutable access by themselves.

Column views cannot escape into stored classes, enums, collections or suspended
generators. Shared access from scoped workers must obey existing worker
eligibility rules; the initial proposal does not add parallel mutation or
automatic partitioning. Row positions are not entity identities: ordered
removal shifts later indices, just as in a dense list.

## Allocation and ownership transaction

An append reserves every affected buffer before moving any field out of the
input row. Only after all reserves succeed may it scatter fields and publish
the new length. The commit phase must be infallible and call no user code.
If a later reserve fails, earlier capacity increases may remain; logical
contents, length and input ownership must not change. Overflow when computing
byte capacities follows the allocation-error contract. Never use independent
fallible pushes after publishing the first column.

The failed append's typed error owns the complete `T`, permitting retry or
explicit destruction. A convenience that discards it can be evaluated later;
do not silently lose an affine owner. The Rust spike checks this transaction
boundary with injected reserve failures; it is not a proof of Plenty's future
allocator implementation.

The scalar-only restriction makes movement and row reconstruction simple.
Supporting affine fields later requires compiler-owned field extraction that
does not introduce unrestricted user partial moves. It must transfer/drop each
field exactly once. Custom class destructors need a whole-row receiver and
defined ordering relative to fields; initially reject such classes instead of
silently skipping or duplicating their destructor.

Storing `str` handles in a column would make **handles**, not string bytes,
contiguous. Packing variable-width payloads needs offsets, invalidation and
mutation contracts of its own. Nested lists, nullable values, enums and recursive
data likewise do not automatically become Arrow arrays. Conversion to/from
ordinary lists or Arrow would be explicit and potentially allocating; no FFI
layout, zero-copy conversion or serialization compatibility is promised.
