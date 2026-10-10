# Collection storage and costs

`collection.rs` supplies static operation signatures. Typed loops and builders
lower to structured operations; Cranelift calls a fixed runtime ABI using
128-bit slots and immutable element metadata. No per-element compilation or
dynamic type inference occurs.

Lists use dense `Vec` storage with constant-time indexing. Inline records, tuples,
and enum payloads occupy contiguous rows; buffer moves relocate private addresses.
Mutation grows buffers in place. Explicit `copy` duplicates owned contents.
Literals and comprehensions build in amortized linear time under ordinary hash
distribution; operation contracts retain their documented allocation failures.

## Hash storage

Dictionary/set hash buckets point to stable row slots. A separate fallibly
allocated buffer uses two machine words per reserved slot (16 bytes on the native
target) for insertion-order links or the dead-row free list. Occupancy does not
depend on payload bits. Removal unlinks a row and repairs its probe cluster by
backward shifts, without moving payloads or scanning the whole table.

New keys reuse free slots at the logical tail; replacement preserves position.
Live-row cursors make traversal, snapshots, copying, formatting, and destruction
follow insertion order without visiting dead rows. Capacity follows peak occupancy
and explicit reservation, and persists until destruction. Hashes are not randomized.

Reservation obtains bucket, link, and row capacity before publishing indices or
transferring owners. Failure preserves contents and order, though capacity may
have grown. Relocation ignores dead rows. A popped inline payload stays readable
until mutation or receiver release; codegen snapshots it before either.

## Narrow native helpers

List length and integer/float/bool reads use `plenty_list_len` and
`plenty_list_scalar_get`, bypassing the generic dispatcher's argument scratch
area. They take a live owner pointer and, for reads, an `i64` index; results
preserve payload bits before narrowing or bitcasting. Codegen does not depend
on `Vec` layout or cached buffer addresses. Named receivers are borrowed without
temporary retain/release; temporary owners are released after obtaining the result.

Managed reads and some nested projections retain the generic ownership path.
String length is cached, UTF-8 scalar indexing scans boundaries, and text iteration
uses a byte cursor. Compiler-generated builder/iterator locals count toward the
256-slot limit. `examples/collection_bench.py` and `examples/collection-bench.md`
provide reproducible workloads and measured costs.
