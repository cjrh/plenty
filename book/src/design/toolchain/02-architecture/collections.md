# Collection storage and costs

`collection.rs` supplies static operation signatures. Typed loops and builders
lower to structured operations; Cranelift calls a fixed runtime ABI using
128-bit slots and immutable element metadata. No per-element compilation or
dynamic type inference occurs.

Lists use dense storage with constant-time indexing. Primitive scalar rows use
their declared widths; other rows use a value slot plus their inline payload.
The buffer is a byte prefix of an aligned `Vec<u128>` allocation. Inline records,
tuples, and enum payloads occupy contiguous rows; buffer moves relocate private addresses.
Mutation grows buffers in place. Explicit `copy` duplicates owned contents.
Literals and comprehensions build in amortized linear time under ordinary hash
distribution; operation contracts retain their documented allocation failures.

## Hash storage

Dictionary/set hash buckets point to stable row slots. A separate fallibly
allocated buffer uses two machine words per reserved slot (16 bytes on the native
target) for insertion-order links or the dead-row free list. Occupancy does not
depend on payload bits. Removal unlinks a row and repairs its probe cluster by
backward shifts, without moving payloads or scanning the whole table.

Each bucket is one 64-bit word. For a table of `2^s` buckets, its low `s` bits
hold the row slot and the bits above hold the low `64 - s` bits of the key's
hash, so storing hashes costs no memory. A probe compares keys only where those
bits match its hash. Growth rebuilds the table from stored bits without reading
or hashing keys, and removal finds a shifted bucket's home from them; both fall
back to rehashing keys only above `2^32` buckets (issue #761). Load stays at most
one half, so the slot field always fits. Copying a collection, set algebra and
`update` still hash each key they insert or look up.

New keys reuse free slots at the logical tail; replacement preserves position.
Live-row cursors make traversal, snapshots, copying, formatting, and destruction
follow insertion order without visiting dead rows. Capacity follows peak occupancy
and explicit reservation, and persists until destruction. Hashes use SipHash-1-3
with a process-wide secret key obtained from OS entropy. The key is shared across
worker threads; dictionary insertion order remains independent of bucket placement.
See the [hashing contract](../../facilities/01-builtins/collections.md) for
initialization, failure behavior, and complexity limits.

Top-level `str`, `bytes`, integer and `bool` keys hash their contents
directly. Tuple keys recursively hash scalar fields, string bytes, and byte-literal
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

Integer, float and `bool` element reads and writes through a list place are
inline machine code, with no runtime call (issue #509). This applies to any
list place: a local, a field (`self.xs[i]`, `a.b.xs[i] = v`), a borrowed
receiver, or an inner row (`grid[i][j] += 1`), in any loop. Codegen loads the
list's first-row address, stored byte count and popped-row flag from the owner
at the offsets in `plenty-runtime/src/list_layout.rs`, which the runtime
asserts against its `repr(C)` `Collection` and `Entries`; a scalar row is 1, 2,
4 or 8 bytes according to its type. One unsigned comparison accepts an index
in `0..len`; any other index takes a cold path that adds the length and checks
again, so a nonnegative index needs no normalization (issue #715). A constant
index, or one known nonnegative, takes the single comparison only, and both
paths join on the row address (issue #720). An access to the same rows,
length and index values as a check earlier in the block, or in the chain of
check and trap-guard blocks leading to it, reuses that check's address, since
it cannot fail if the first passed (issue #721). It then
loads or stores the declared bytes. Reads preserve payload bits before
narrowing or bitcasting, and writes store the same bits a write through an
element reference would. An index out of bounds calls
`plenty_trap_list_index`, which reports the index as written, the length and
the site, as the generic path does. The failure block stores the index and
length to the function's scratch slot and passes its address: as register
arguments, they would pin the loop's values to the call's argument registers
and add moves to every iteration. The length of a borrowed scalar list is
read the same way. The list's loan keeps the rows in place for each access.
Outside the loop caches below, codegen does not cache the address across source
operations, so growth between accesses is observed.

A `for` or `while` loop caches the length and first-row address of a named
scalar list, or of a scalar-list field of a named class value
(`self.items[i]`), defined before it (issues #429, #715 and #717). The frontend
emits the header reads before the loop and lowers `xs[i]` in the loop to
`ListScalarGetView` or `ListScalarSetView`, which take the cached values as
operands. A list of any other element type gets the same cache for the row
references that field access and element references take (`bodies[i].x`,
`&mut bodies[i]`): `ElementRefView` takes the cached values and computes the
row address inline (issue #739). An immutable shared reference cannot change during the loop. For an
owned list or owner, an exclusive reference, or a retargetable binding, the
frontend checks the finished loop's operations against a list that cannot
resize or replace any list (arithmetic, control flow, loan bookkeeping, `?`,
scalar row access, row references, length queries, view construction, calls, and no write to
the binding). A direct call is allowed when none of its arguments can resize
the list: Plenty has no globals, so a callee reaches a list only through its
arguments. Shared borrows and `&mut` passed to a slice view cannot resize it,
nor can `&mut` to an unrelated owned local. The loop falls back for a `&mut`
list or class argument, including a method receiver, that reaches the cached
list or its owner or that is borrowed from a reference parameter; for a
by-value argument that holds references; for indirect and closure calls; and
for any other operation. The fallback rewrites those accesses to `ListScalarGetRef`,
`ListScalarSetRef` and `ElementRef`, which read the header at each access. The cache is held in
ordinary locals rather than in loads marked `readonly`: Cranelift treats a
`readonly` load as valid for the whole function, which a later `append` would
violate.

The length of a borrowed list of any element type is read from the header
inline: the stored byte count divided by the constant row size, a shift when
it is a power of two, minus the popped-row flag. The length of an owned
temporary uses `plenty_list_len`, and indexing an owned temporary scalar list or iterating one by value uses
`plenty_list_scalar_get`, bypassing the argument block of a
[runtime operation](runtime.md#entry-symbols). These helpers take a live owner
pointer and an `i64` index. Named receivers are borrowed without temporary
retain/release; temporary owners are released after obtaining the result.

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
operand pushes that run no code and release nothing (loads, literals,
references, last-use moves, and stack swaps); it removes each copy's retain and
skips its release. Any other op, including a conditional or call that computes
an operand, ends the tracking.

Runtime operations that only read their operands read every such copy in
place, whatever its type: dictionary `get`, `get_mut`, and indexing; `in` on a
list, dictionary, set, range, or text; list `find`, `rfind`, and `count`; set
subset, superset, and disjoint tests; and `Bytes` reads, writes, and scans that
do not resize the buffer. In `counts.get_mut(&key)` the key is not counted, nor
is a dictionary read through a shared reference, and a `Bytes` method's read of
`self._owner` counts nothing. These operations run no Plenty code (keys hash
and compare structurally) and release or replace no stored value. Operations
that can, such as `pop`, `discard`, and inserts, keep the copy's count, because
a stored value they release may be the copy's source.

Inserts store their operands without adjusting counts (issue #762). Every
operand codegen passes is owned, whether its read moved it or counted a copy,
so `insert`, `add`, `append`, and a `get_or_insert` miss make that count the
collection's, and a failed insert releases it once. A `get_or_insert` hit
keeps the stored entry: the runtime releases affine operands, and codegen
tests a hit flag in the result and releases copyable ones inline. A mutation
of a receiver through `&mut` (`counts.insert(k, v)` on a parameter or local,
`pop`, `discard`, `clear`, set updates, `reverse`, `sort`, and so on) reads the
collection's owner in place under the exclusive loan, so the receiver is
neither retained nor released. `clear`, `reverse`, `sort`, `swap`, and the set
updates still return a retained alias of the receiver, which the caller
releases (issue #813).

An element reference (`&xs[i]`, `&mut xs[i]`, `&mut grid[i][j]`, and the
implicit one under `bodies[i].x`) of any element type checks the index inline
like scalar access and computes the row's address, `rows + index * row_bytes`,
where `row_bytes` is the element's constant row size (issue #739). An index
out of bounds calls `plenty_trap_list_index` with the same message as other
list indexing. That address is the reference; reads and writes through it are direct loads and stores. Dictionary
element references still use a runtime operation.

A borrowed list loop (`for x in &xs`, `for x in &mut xs`, `reversed(&xs)`, or a
loop over a list reference) iterates through the reference without copying the
list's owner. Before the first iteration it reads the length and the
first row's address from the header and uses the element's constant row size
as the distance between rows. Each element reference is
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
