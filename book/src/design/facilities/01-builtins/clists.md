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

Field replacement evaluates the RHS before its index, following ordinary
assignment. Augmented assignment evaluates the index, reads its current value,
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

Column loops load the pointer and length before iteration and issue native
scalar loads inside the loop. Checked arithmetic and floating-point rules are
unchanged. Indexed field access and row copying currently use runtime helpers;
whole-row workloads can be much slower than ordinary lists. The current helper
overhead is tracked in issue #432; the [measurements](../../../proposals/columnar-list/native-implementation.md)
include indexed and reconstructed whole-row comparisons. The layout makes future
SIMD and parallel scans possible, but choosing `clist` does not automatically
vectorize or parallelize a calculation.

See the [tutorial](../../../tutorial/06-data/11-columnar-records.md) and the
[design rationale and measurements](../../../proposals/columnar-list.md).
