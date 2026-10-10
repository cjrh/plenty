# Collection implementation and current costs

`collection.rs` gives each operation one static signature. The frontend lowers
loops and comprehensions into typed locals and structured loops. Cranelift calls
a fixed native collection ABI with 128-bit slots; immutable metadata encodes the
already known element types. Metadata supports native storage/equality/printing, not
dynamic type inference. No per-element compilation or trait instantiation occurs.
Every program links the same precompiled Rust runtime archive; no runtime source
is compiled for individual programs.

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
