# Columnar lists

`clist[T]` stores each field of `T` in a contiguous column of native-width
values. Choose it for repeated scans of a few fields in many rows. Ordinary
`list[T]` remains useful for general objects and whole-record access.

`T` must be a fully defined, nonempty class containing only `bool`, signed or
unsigned 8/16/32/64-bit integers, `f32`, or `f64`. Custom destructors and fallible
initializers are rejected. Strings, nested classes, enums, nullable fields,
collections and references are not supported. Creating a clist or copying a
row requires access to every field; field selection respects module privacy.

## Construction and mutation

| Operation | Result and behavior |
| --- | --- |
| `clist[T]()` or `clist[T].new()` | Empty owner, without allocating |
| `clist[T].with_capacity(n)` | `Result[clist[T], AllocError]` |
| `len(items)` or `items.len()` | Live row count as `i64` |
| `items.capacity()` | Row capacity as `i64` |
| `items.reserve(additional)` | `Result[(), AllocError]`; capacity for `len(items) + additional` rows |
| `items.append(row)` | `Result[(), AppendError[T]]`; moves the row |
| `items[i].field` | Checked scalar read; negative indices count from the end |
| `items[i].field = value` | Checked scalar replacement |
| `items[i].field += value` | Numeric augmented assignment; evaluates the index once |
| `items.copy_row(i)` | Allocation-free `Option[T]`, independent of subsequent writes |
| `items.column(T.field)` | Local, one-shot `Iter[FieldType]` borrowing the owner |

Capacity growth allocates replacement storage, copies each column, and publishes
the new storage only after success. Failure preserves length, capacity and all
existing values. Negative or overflowing capacity requests return
`AllocError.CapacityOverflow`. Capacity may exceed the requested size.

`AppendError.OutOfMemory(row)` and `AppendError.CapacityOverflow(row)` preserve
the complete submitted row for retry. Successful append moves its scalar fields
into the columns without rerunning an initializer. An append within reserved
capacity allocates nothing. The empty owner and all live rows are cleaned up on
every ordinary exit; moving a clist transfers its storage.

Field replacement evaluates the RHS before its indices, following ordinary
assignment. Enclosing owner indices precede the row index, and each is evaluated
once. Augmented assignment evaluates these indices, reads its current value,
then evaluates the RHS under a shared owner loan before writing. Out-of-bounds
indices trap at the source position; `copy_row` returns `Nothing` instead.

## Column iteration and ownership

`T.field` is a compile-time selector accepted only by `column`. It is not a
string lookup or a general reflection value. Each column plan borrows the entire
clist until consumed. While that loan is live, reserve, append, field writes,
movement and destruction of the owner are rejected. Several shared column plans
can be zipped together. Their lengths and row order agree.

Columns yield **scalar values**, not `&Field` references. The source remains
borrowed, but each element is loaded directly from its native-width buffer.
Ordinary Plenty references address tagged value slots; treating packed bytes as
those slots would be invalid. Use scalar callback parameters with existing
`map`, `filter`, `fold`, `zip` and other [local pipeline operations](../../language/09-iteration/06-pipelines.md).
Recreate a column plan for a second traversal. Plans cannot escape into stored
values or suspended generators.
Column iteration is currently rejected inside generator functions, even if the
scan would finish before a `yield`.

`items[i]`, `&items[i]`, packed field references and direct row-method calls are
unsupported: there is no contiguous `T` at that row position. `copy_row` creates
an ordinary owned `T` on which methods can run; writes to that copy do not write
back. Whole-clist `copy`, row removal, clear, mutable column views and general
row iteration are not part of this initial interface. Equality compares row
values; formatting reports `clist(len=N)` without materializing rows.

## Layout and cost

The current implementation uses one allocation for an owner header and all
columns. Every column contains tightly packed elements (`bool` uses one byte).
Capacity rounding aligns column starts; this is an internal layout, not an
Arrow, Parquet, C ABI or serialization contract. Growth copies all columns,
and can temporarily hold both old and new allocations.

Column loops load the pointer and length before iteration. Checked arithmetic and floating-point rules are
unchanged. Indexed field reads and writes use checked native-width loads and
stores. `copy_row` checks its index once, then gathers the known fields directly
into fresh ordinary row storage without runtime schema dispatch. Each access
observes the current owner, so growth and mutation between accesses are safe.
Whole-row workloads can still cost more than ordinary lists: gathering separated
fields and moving the reconstructed object have costs of their own. The
[access measurements](../../../proposals/columnar-list/indexed-access.md)
compare these paths with the initial implementation.

## Automatic SIMD counts

With `--release`, a column of `i32` or `u32` followed by one `filter` and
`count()` uses four-lane SIMD when the predicate is a comparison of its borrowed
argument against a constant of the same type (`==`, `!=`, `<`, `<=`, `>`, `>=`).
Both a named function and a closure without captures are recognized. For example:
`items.column(T.score).filter(def(x: &i32) -> bool: *x > 5i32).count()`.
The ordinary scalar program remains valid without `--release`.

The compiler proves this small pattern from the checked operations; it requires
no user annotation, temporary collection, allocation, aligned column start or
lane-count bookkeeping. Full groups use unaligned-safe vector loads; the scalar
loop handles the remaining zero to three rows. Counts use 64-bit accumulators,
including for columns larger than a 32-bit lane counter could represent.
The generated instructions use the existing x86-64 SSE2 baseline, so this slice
needs neither a newer CPU nor runtime feature dispatch.

Captures, checked arithmetic in predicates, additional adapters, other scalar
types and other terminals retain scalar execution. Side effects, traps and
floating-point order are unchanged. This is a bounded optimization, not general
auto-vectorization; parallel scans remain unimplemented. See the
[validation and measurements](../../../proposals/columnar-list/simd-counts.md).

See the [tutorial](../../../tutorial/special-purpose/columnar-lists/index.md) and the
[design rationale and measurements](../../../proposals/columnar-list.md).
