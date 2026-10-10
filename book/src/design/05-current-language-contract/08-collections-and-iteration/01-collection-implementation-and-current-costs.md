# Collection implementation and current costs

`collection.rs` gives each operation one static signature. The frontend lowers
loops and comprehensions into typed locals and structured loops. Cranelift calls
a fixed native collection ABI with 128-bit slots; immutable metadata encodes the
already known element types. Metadata supports native storage/equality/printing, not
dynamic type inference. No per-element compilation or trait instantiation occurs.
Every program links the same precompiled Rust runtime archive; no runtime source
is compiled for individual programs.

List length and integer/float/boolean element reads use narrow native helpers
(`plenty_list_len` and `plenty_list_scalar_get`) instead of the collection
dispatcher and its 128-bit argument scratch area. The helper ABI takes a live
list owner pointer and, for reads, an `i64` index, returning one 64-bit word.
Scalar results preserve their payload bits before codegen narrows or bitcasts
them to their declared type. Both helpers read the current private row storage;
codegen depends on neither Rust `Vec` layout nor cached buffer addresses.
Negative and extreme indices use the dispatcher's existing bounds normalization
and `index out of bounds` diagnostic.

Named list receivers, including field/element projections and reference
parameters, are borrowed through each observation. Scalar list iteration also
borrows its compiler-owned source local. These paths create no temporary
collection owner and emit no receiver retain/release calls. Owned temporary
receivers use the same helpers but are released after the result is obtained.
Nested-list projection can still call the generic element-reference operation;
managed element reads retain the generic ownership path. Stores, hash probes,
string character indexing, and front removal are unchanged. Scalar class field
loads already have separate direct lowering; managed/inline field ownership
and snapshots can still require runtime calls.

The reproducible `examples/collection_bench.py` suite covers reads, writes,
length, nested lists, hash hits/misses, and front removal at two input sizes.
`examples/collection-bench.md` records compiler options and measured costs.

Ranges carry 32 bytes of inline bounds/step/length data. Runtime argument slots
refer to live caller-owned data; stored range values (including inside standard
sums) keep their data in the owning local, record, generator frame, or collection
buffer. Typed storage slots are 16 bytes normally and 48 bytes when they contain
a range. Buffer relocation repairs those private addresses. Range temporaries
and returned ranges never point into expired storage or allocate a separate owner.
Calls passing or returning inline range values currently use ordinary calls
rather than native tail calls, so the caller's temporary storage stays live.

The Rust runtime uses `Vec` storage and hash tables with ordered entries for
dictionaries and sets. Private builders append in place; literal and comprehension
construction is amortized linear under ordinary hash distribution. Public updates
also mutate in place; repeated `append` no longer copies existing contents.
Only explicit `copy` duplicates owned contents. Compiler-emitted type metadata caches
whether a type owns mutable contents.

Dictionary/set hash buckets refer to stable row slots. Two machine words per
reserved slot (16 bytes on the native 64-bit target), in a separate fallibly
allocated buffer, link live rows in insertion order; dead rows use those words
as a free list. Occupancy never depends on key/value bits. Removal unlinks a row
and repairs its linear-probe cluster with backward shifts, without allocating,
moving payloads, or scanning the whole table. Newly inserted keys reuse free
slots at the tail of the logical order; replacement keeps its position.
No compaction threshold or periodic dead-slot scan is needed. Capacity follows
peak live occupancy and explicit reservation, and is retained until destruction.

Native hash iteration carries a private slot cursor, so traversals, snapshots,
copying, formatting, and destruction visit only live rows in order, even after
heavy deletion. Lists remain dense, with constant-time indexing. Reservation
obtains bucket, link, and row capacity before publishing any new indices or
transferring owners. On failure, contents and order remain unchanged, though
capacity may have grown. Row relocation repairs live inline payload addresses;
dead rows never participate in relocation or cleanup. A popped inline payload
remains readable until the next mutation or receiver release; codegen snapshots
it before either occurs.

Class instances, tuples, and enum values are stored inline in the buffer's rows,
so a `list[Point]` keeps its points contiguous and building it allocates only for
buffer growth. Inserting an affine element moves it into the buffer.
Heap payloads are reference counted. Inline values retain/release only their
active payload and fields. Replacing a local releases its previous
value; scope/function exits release remaining owners. Collection buffers are
reclaimed along with objects; type metadata lives in immutable program data.
Private expression temporaries
can remain until their enclosing scope exits. Retained helper operands are
implementation details, not permission to create source-visible mutable aliases.

String length is cached; scalar indexing scans UTF-8 boundaries. String iteration
uses a private byte cursor and is linear in byte length. Keys/values lists are snapshots. Hashes are not randomized. These are
explicit initial runtime limits, not promises of Python's complete container API.
Compiler-generated builder and iterator locals count toward the 256-slot limit.
