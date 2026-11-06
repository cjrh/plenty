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
would have owned.

## Allocator provenance

Object allocations carry a private prefix identifying their allocator, leaving
generated-code header and payload offsets unchanged. Freeing uses that identity,
never a later global choice. Internal allocator tables must live for the process.
Ordinary objects select the global allocator; `Vec`-backed container buffers use
it directly. Public allocator selection and scoped allocator lifetimes are unsupported.

Checked buffer growth and partial-construction cleanup implement the
[recoverable allocation contract](../../language/08-errors/02-allocation-and-traps.md).
The compiler's `runtime-checks` feature enables the embedded runtime's
`allocation-checks` instrumentation for native integration tests. Allocation-free
regions count successful allocations, including those freed inside the region;
failed attempts do not count. Failure injection is separate. See
[runtime validation](runtime.md#validation).
