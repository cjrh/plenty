# Memory and destruction

Each live managed operand, local, field, and capture has an owner. Moving an affine
value clears its source slot. Loading a copyable inline value copies its storage
and retains contained strings. Stores stage the new value before releasing the
old owner. Source cleanup order is defined by
[destruction](../../language/05-ownership/07-destruction.md), not reference counts.

Runtime objects share `{atomic u64 refs, destroy_callback}`. Heap objects begin
with one reference; literals use an immortal count. A
[`Shared[T]`](../../facilities/01-builtins/shared.md) handle is a box whose
count is shared by its source-level owners. Native helpers borrow inputs
and return owned managed results, including retained projections and builder aliases.
Buffers have explicit owners; type metadata is immutable program data.

A class instance owns its fields in a record inside its owner's slot; the slot's
value word is the record's address, which moves relocate. A class field, tuple
component or multi-field variant field of class type is stored flat inside its
owner's record, so moving the owner copies bytes without relocating that field;
an all-zero flat record marks a moved-out or released field. A one-field class
without `__del__` has no record: the instance is its field, so moving it or
moving the field out copies the field's bits and releases nothing. See
[value representations](values.md).

Retain uses relaxed atomic updates. Final release acquires preceding releases
before destruction; the final thread owns the destruction queue entry. This
protects ownership counts, not concurrent mutable payload access or foreign
resource transfer. Source permissions still come from borrow and worker checks.

### Sole-owner release

Release first loads the count. An immortal count returns. A count of 1 skips
the locked decrement: an acquire fence follows the load and the object is
destroyed, leaving the dead count at 1 until the destruction queue reuses the
word. Other counts take the release decrement.

This is sound because the releaser consumes a counted reference it owns, so
the count is at least 1 and other counted references or borrows account for
the rest:

- Every count operation by another thread comes from a counted reference or a
  borrow of one. A borrow of the releaser's own reference ends before the
  release, through a scoped-worker join or a job or channel handoff, which
  orders that thread's retains before the load. By coherence the load then
  reads their increments or a later value, so it cannot read a stale 1.
- When the load reads 1, no other counted reference exists, and no new one can
  appear: creating one needs the releaser's reference, which it owns
  exclusively. A retain racing with this release would already race with
  destruction under an atomic decrement, so borrow and worker checks exclude it.
- The 1 was written by the initial store or by another owner's release
  decrement. The acquire fence synchronizes with that decrement, so the other
  owner's payload use happens before destruction, as under the atomic path.
- [`Shared[T]`](../../facilities/01-builtins/shared.md) handles each own a
  count, so two live handles keep it at 2 or more. Borrowed contents are
  released only when the last handle destroys the box or under a mutex.
- The destruction queue writes its link only after the release has decided to
  destroy, as before. Immortal literals keep `u64::MAX` and are never written,
  so read-only literal data stays valid. Copies into another actor heap check
  the count only for the immortal value.

Retain has no such path. It also runs through shared borrows, which carry no
count: a scoped worker and its parent copying one string, or two jobs reading
the same `Shared[list[str]]`, can both read 1, and a plain store of 2 would
lose an increment. Retain therefore always uses an atomic update. Compiled
code changes counts only by calling `plenty_retain` and `plenty_release`.

An iterative queue destroys owned graphs without recursive native-stack growth.
A record, list, or box being destroyed hands each heap child to the queue and
resumes its walk after that child is gone, which keeps cleanup in declaration
order. A string owns nothing and runs no user code, so the walk frees it in
place instead. A class whose fields own only strings, at any depth and without
`__del__`, is released in one step from the string offsets its descriptor
records; see [value representations](values.md).
Inline generator cleanup uses the count handoff but finishes synchronously while
its enclosing storage is live. Private helper aliases do not authorize mutable
source aliases. Traps end the process without unwinding language scopes.

### Box destruction

When a box's content releases without running code, the free order of its
children cannot be observed. Its descriptor then carries a compiled release of
the content slot, and the runtime calls that function instead of walking the
content's descriptor. The compiler builds one for each boxed type that is:

- a string, collection, box, or another value released with one
  `plenty_release`;
- a sum such as `Option[Box[T]]` in which one variant owns a single such
  value; or
- a class without `__del__` whose fields are of those kinds or own nothing.

The function reads each owned child at its fixed offset and passes it to
`plenty_release`. During destruction that call only queues the child, so deep
trees still drop without growing the native stack. Content whose release runs
a destructor keeps the ordered walk described above.

`Box(value)` for content with inline storage, such as a class record, asks
the runtime only for the allocation. Compiled code then copies the content into
the box's slot. If the allocation fails, the runtime releases the value the box
would have owned. The slot is not zeroed first, just as a local's stack slot is
not: the copy writes the content and every reader of the box reads only what
was written. A class record is stored flat, with no value word, as a class
field is; an all-zero record is an empty box content. Other content keeps the
value word and the payload that word names, such as the current variant's
fields of a sum, and readers read the word first and then only that payload.
Bytes past a smaller sum variant stay unwritten and are never read. A protocol
box keeps the value word even for a class, because its adapters receive the
content slot without the type; its nonzero padding word, which holds the
adapter or table, tells the runtime which layout the box uses.

## Allocator provenance

Runtime objects and container buffers both come from the global allocator, and
an object allocation carries no prefix before its header: with only one
allocator, freeing needs no record of which one created an object. A box is a
16-byte header, its descriptor word and a padding word, then its content: a
flat class record, or a slot. A binary-trees node is a 64-byte box in a 64-byte
pool block (C: a 32-byte malloc chunk); with glibc malloc it was an 80-byte
chunk.
Public allocator selection and scoped allocator lifetimes are unsupported. Adding
them (backlog B13) must record provenance again without enlarging objects of the
global allocator; a 16-byte prefix made each binary-trees node a 112-byte malloc
chunk rather than 96 (issue #814), before the record's value word was also
dropped.

### Small-block pool

On Linux x86-64 and ARM64, a global allocation outside an actor heap or arena
whose layout is at most 256 bytes with alignment at most 16 comes from the
runtime's size-class pool rather than malloc (issue #814). Strings, small list
buffers, boxes and runtime records all qualify. Larger or more aligned layouts
still use malloc.

- Each class is a multiple of 16 bytes. The class depends only on the layout,
  which every free and reallocation also receives, so blocks carry no header.
  Reallocating within a class keeps the block; moving between classes, or
  between the pool and malloc, copies.
- Blocks are carved from 64 KiB slabs aligned to their size. Each thread caches
  up to about 16 KiB of free blocks per class. A block freed on another thread,
  such as a value moved to a worker or a `Shared` value whose last owner is a
  worker, joins the freeing thread's cache. A full cache returns half its blocks
  to their slabs under one lock, and an exiting thread returns all of them.
- A slab counts its blocks in use or cached. An emptied slab can serve any
  class. A slab that stays empty for a second is unmapped the next time another
  slab empties, so a program that frees and rebuilds a large structure reuses
  its slabs instead of mapping them again. A program that frees memory and then
  stops allocating keeps those slabs mapped.
- Actor heaps and arenas keep their own free lists. The pool sits beneath the
  `allocation-checks` accounting allocator, so allocation counts, allocation-free
  regions and failure injection see every reuse. In `plenty test` and
  `--test-hooks` builds, which inject failures by wrapping malloc, each pooled
  allocation spends the same budget as a malloc call. A slab that cannot be mapped
  is an allocation failure, reported as `AllocError` like a failed malloc.

At binary-trees size 21, peak RSS fell from 657 MB to 527 MB (C: 264 MB).
At size 18, user-space instructions fell from 48.1 to 33.0 billion. A list of
5 million boxed nodes, built and freed four times, fell from 392 MB to 314 MB
and from 13.9 to 9.2 billion instructions. Each allocation still passes
through the global allocator's actor-heap and arena checks.

Checked buffer growth and partial-construction cleanup implement the
[recoverable allocation contract](../../language/08-errors/02-allocation-and-traps.md).
The compiler's `runtime-checks` feature enables the embedded runtime's
`allocation-checks` instrumentation for native integration tests. Allocation-free
regions count successful allocations, including those freed inside the region;
failed attempts do not count. Failure injection is separate. See
[runtime validation](runtime.md#validation).
