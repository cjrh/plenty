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
and explicit reservation, and persists until destruction. Hashes use SipHash-2-4
with a process-wide secret key obtained from OS entropy. The key is shared across
worker threads; dictionary insertion order remains independent of bucket placement.
See the [hashing contract](../../facilities/01-builtins/collections.md) for
initialization, failure behavior, and complexity limits.

Tuple keys recursively hash scalar fields, string bytes, and byte-literal
contents using their static metadata. Inline storage addresses never participate in the hash, so copied
keys, relocated rows, lookup temporaries, and newly constructed equal strings
all retain the same hash. Tuple hashing and equality allocate nothing.

`std.deque` builds a ring buffer in Plenty over a private list of optional
payloads. End removal replaces a slot with `Nothing` and transfers its previous
owner. Growth doubles the slot count and initializes a replacement buffer
before moving live entries; list removal behavior is unchanged.

`std.collections` keeps each binary heap in a private list and restores heap
order with `swap`, so sifting never copies or drops a payload.

`std.bytes` does not use collection slots. Its private owner holds a contiguous
one-byte-per-element buffer, reached only through range-checked runtime
operations that never expose an address. See [std.bytes](../../stdlib/bytes.md).

Reservation obtains bucket, link, and row capacity before publishing indices or
transferring owners. Failure preserves contents and order, though capacity may
have grown. Relocation ignores dead rows. A popped inline payload stays readable
until mutation or receiver release; codegen snapshots it before either.

## Narrow native helpers

List length and integer/float/bool element reads and writes use
`plenty_list_len`, `plenty_list_scalar_get`, and `plenty_list_scalar_set`,
bypassing the generic dispatcher's argument scratch area. This applies to any
list place: a local, a field (`self.xs[i]`, `a.b.xs[i] = v`), a borrowed
receiver, or an inner row (`grid[i][j] += 1`). The helpers take a live owner
pointer and an `i64` index; reads preserve payload bits before narrowing or
bitcasting, and writes store the same bits a write through an element reference
would. Bounds traps report the same site and message as the generic path. Codegen does not depend
on `Vec` layout. Named receivers are borrowed without
temporary retain/release; temporary owners are released after obtaining the result.

A borrowed list loop (`for x in &xs`, `for x in &mut xs`, `reversed(&xs)`, or a
loop over a list reference) iterates through the reference without copying the
list's owner. Before the first iteration it reads the length with
`plenty_list_len`, the first row's address with `plenty_list_rows`, and the
distance between rows with `plenty_list_row_bytes`. Each element reference is
then `rows + index * row_bytes`, the slot a generic element reference names, so
a scalar read is one load with no call. The loop holds the list's loan from its
first condition to its last body use; no mutation can grow, shrink, or move the
rows meanwhile, which is what makes the cached length and address valid. Element
values keep reference semantics: writes through `&mut` land in the list, and an
element can be passed on or returned under the list's loan.

Managed reads and some nested projections retain the generic ownership path.
String length is cached, UTF-8 scalar indexing scans boundaries, and text iteration
uses a byte cursor. Compiler-generated builder/iterator locals count toward the
256-slot limit. `examples/collection_bench.py` and `examples/collection-bench.md`
provide reproducible workloads and measured costs.
