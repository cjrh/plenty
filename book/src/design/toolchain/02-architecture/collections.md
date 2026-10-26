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
bypassing the argument block of a [runtime operation](runtime.md#entry-symbols). This applies to any
list place: a local, a field (`self.xs[i]`, `a.b.xs[i] = v`), a borrowed
receiver, or an inner row (`grid[i][j] += 1`). The helpers take a live owner
pointer and an `i64` index; reads preserve payload bits before narrowing or
bitcasting, and writes store the same bits a write through an element reference
would. Bounds traps report the same site and message as the generic path. Codegen does not depend
on `Vec` layout. Named receivers are borrowed without
temporary retain/release; temporary owners are released after obtaining the result.

Text and `bytes` queries that return a scalar (`len`, `byte_len`, `isascii`,
indexing, `ord`) call one narrow helper each, such as `plenty_text_len` or
`plenty_bytes_item`; a `bytes` literal index calls `plenty_byte_str_item`.
`plenty_text_ord` returns the code point, or `u64::MAX` when the text is not
one scalar, and codegen builds the `Result` inline. Results, bounds traps, and
sites match the generic operations'. A query of a borrowed place reads through
the reference. A query of an owned local or full-expression temporary reads the
value in its slot: codegen removes the retain the load added and skips the
matching release, since nothing runs between the load and the helper call.

`startswith`, `endswith`, `find`, `rfind`, and `count` call
`plenty_text_starts_with` and its siblings with both texts in registers;
`plenty_text_find` and `plenty_text_rfind` return `u64::MAX` for no match, and
codegen builds the `Option` inline. These searches, `in`, `==`, `<`, and the
runtime operations whose operands are all texts or scalars (`replace`,
`upper`, string concatenation, and so on) read in place each text operand that
its source still holds: a copy loaded from a local or read through a
reference, or a string literal. Codegen tracks these copies across the
operand pushes that run no code and write no slot (loads, literals,
references); it removes each copy's retain and skips its release. Any other op,
including a conditional or call that computes an operand, ends the tracking.

An explicit element reference (`&xs[i]`, `&mut xs[i]`, `&mut grid[i][j]`) of any
element type calls `plenty_list_row(list, index, site)`, which checks the index
like the scalar helpers and returns the row's address. That address is the
reference; reads and writes through it are direct loads and stores. Dictionary
element references still use a runtime operation.

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

`std.bytes` element reads and writes and `Bytes.len` use `plenty_bytes_item`,
`plenty_bytes_set_item`, and `plenty_bytes_len`. `Bytes.push` calls
`plenty_bytes_push` with its storage owner read through the exclusive field
reference, so appending a byte neither retains nor releases the owner and
reaches the allocator only when the buffer grows. Text `len`, `byte_len`,
`isascii`, scalar indexing, and the forward and reverse iteration cursors use
`plenty_text_*` helpers. A text or byte owner reached through a place (a local,
parameter, field, or row) is read through a shared reference under the
caller's loan, so the query neither retains nor releases it; other receivers
are released after the call. A scalar text result is always inline. Indexing
traps report the same site and message as the generic path.

Managed reads and some nested projections retain the generic ownership path.
String length is cached, UTF-8 scalar indexing scans boundaries, and text iteration
uses a byte cursor. Compiler-generated builder/iterator locals count toward the
256-slot limit. `examples/collection_bench.py` and `examples/collection-bench.md`
provide reproducible workloads and measured costs.
