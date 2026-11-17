# Retained allocator storage

`plenty-runtime/src/allocator.rs` implements the native storage substrate for
B13's builtin-owner integration. **It has no compiler binding or public Plenty
API yet.** Compiled programs still select their existing allocation paths, and
the experimental Plenty module is unchanged. Native constructors now exercise
selected-provider storage for ordinary box/list representations; this is not
completion of language-visible builtin allocator support.

## Thread contract decision

Builtin allocator handles are to support safe cross-thread use from the outset,
without allocator type parameters on `list[T]` and `Box[T]`. Their current worker
checks inspect element types; hiding a thread-confined provider behind an
otherwise worker-eligible list would bypass those checks.

The native substrate therefore retains only `Send + 'static` Rust providers and
serializes their allocation and release callbacks. This is not yet a certificate
for compiled Plenty callbacks: their state, effects and destruction also need
compiler verification. Thread-confined Plenty providers remain supported by the
[experimental generic owner](../../stdlib/experimental-owned-list.md).

## Explicit selection and bootstrap

An `Allocator` handle contains either a retained provider-state pointer or the
explicitly selected system root. Obtaining or cloning the system root allocates
nothing. There is no current/default allocator and no fallback after provider
failure.

Creating a provider handle requires an explicit bootstrap allocator. It supplies
the complete, aligned state allocation, including the reference count, callback
table, mutex and provider payload. A failed bootstrap returns both the allocation
error and the original provider. The state retains its backing until after the
provider is destroyed; no `Arc` or hidden system allocation supplies that state.

The system root calls the system allocation primitive directly, not Rust's
ambient global allocator router. Consequently, an active actor heap or scoped
arena cannot silently become its allocator. Allocation-check builds still count
these allocations and inject failures. This path does not use the runtime's
small-block pool.

## Owned blocks and provenance

A `Block` is an affine initialized byte range. Checked slice access and splitting
cannot create overlapping mutable owners. Even empty ranges retain their backing.
Reference counts are atomic, so disjoint ranges can move to different workers.
Payload and provider-state layouts can request alignment above eight; tests
include 4096-byte alignment.

Each backing allocation contains a private header before its exposed payload.
For a custom provider, that header retains the selected allocator and the
original block returned by its callback. The callback request includes this
metadata and alignment padding, not just payload bytes. Extent and alignment
are validated before writing the header. Oversized blocks are accepted.

Final block release moves its provenance out before freeing or clearing its
storage. A custom provider receives its original block, with the entire range
cleared, while its state remains alive. Clearing prevents typed padding and
stale moved-owner representations from reaching safe byte readers. The provider
may cache or drop the returned block. Its final destructor runs only after the
last handle and backing reference are gone, potentially on another worker.

A trusted typed consumer must keep byte views private while it holds initialized
values, and destroy those values before dropping its block. This substrate does
not itself invoke arbitrary element destructors.

## Callback locking and boundaries

One mutex protects each provider. Callbacks on different threads serialize;
callbacks into distinct parent providers may nest. A non-reused, allocation-free
thread identifier detects same-thread recursion before locking, which traps
rather than deadlocking. Poisoned provider locks also trap. Cyclic dependencies
between different providers are not a deadlock-free contract.

This is a Rust-internal interface, not a raw callback ABI for packages. In
particular, `Send` does not prove that a compiled callback cannot perform
nonlocal actor-trap recovery through a held Rust lock. No such callbacks are
connected here. Actor containment and the existing experimental non-atomic
backing blocks are not certified by these tests.

## Native builtin owner paths

`aggregates/allocator_owners.rs` constructs ordinary plain boxes and dense lists
using a selected allocator. Both the list object and its row buffer use that
provider. Empty lists retain it too. These constructors are internal and are not
called by compiler lowering yet.

A private prefix before each selected object's ordinary header owns its backing
block and type descriptor. Ordinary box content offsets and the compiler-visible
list descriptor, rows, byte count and removal flag offsets remain unchanged.
Legacy objects have no such prefix. `Entries` carries an optional pointer after
its ABI prefix. Selected-buffer bookkeeping is allocated separately through the
selected provider, never a global `Box`; it owns the row block and retained
handle. This adds one pointer to legacy collection bookkeeping and an extra
provider allocation to selected lists, including empty lists. These costs have
not been benchmarked.

Selected row growth allocates before moving bytes. Failure preserves live rows,
capacity and their address. Success repairs nested inline payload addresses
before returning the old buffer. Row widths remain the existing dense scalar or
slot layouts. Raw bytes are not reconstructed as a `Vec` on this path; the
selected block owner releases them. Partial list construction releases any
reserved buffer if allocating the object fails.

A selected box's allocation failure leaves its input with the native caller.
Successful construction moves inline payloads into the box. Taking the content
clears its slot and releases the object through its recorded provider.

Selected owners use the ordinary iterative destruction queue, retaining their
prefix and row buffers until children finish. In particular, selected boxes do
not use the legacy fast child-release shortcut: even a child with no language
`__del__` can have an observable allocator release callback. The parent's provider
must not receive its storage ahead of those children. This is covered by a test
whose descriptor explicitly offers the shortcut.

Copying, slices, stable-sort scratch, adapted protocol/callable boxes and other
allocation sites are not routed by these constructors. Their migration remains
separate from storage growth and destruction.

## Evidence

Thirteen native storage tests cover zero-sized and aligned storage, disjoint ranges,
provider lifetime, nested bootstrap ownership, failed bootstrap with returned
state, independent failure without fallback, overflow before callback entry,
cache reuse, concurrent callbacks, final destruction on a worker, malformed
providers, recursion traps and explicit system allocation inside an ambient
arena. Trap subprocesses have deadlines. Successful paths pass Valgrind checks
for invalid access and definite, indirect and possible leaks.

Eleven additional native owner tests cover object/buffer routing, failed growth
and partial construction, instrumented system failures, nested records with
ranges and owned child boxes, extracted-value cleanup, dense scalar ABI offsets,
empty owners, take, final release on a worker and child-before-parent release.
A 10,000-node selected box graph drops on a 128 KiB stack. The combined allocator
checks pass Valgrind, including the new owner paths.

These are Linux x86-64 correctness results, not cross-platform qualification or
performance measurements. The metadata and clearing costs are deliberately not
optimized. Implementation priorities remain in
[B13](../../../backlog.md#b13-explicit-allocator-implementation-sequence).
