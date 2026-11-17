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

## Compiler admission groundwork

A private `ThreadOp::AllocatorProvider` marker names provider state and its
allocation/release functions. It emits no code and has **no frontend producer**.
It reuses worker certification without requiring a worker spawn: state must be
heap-storable and contain no hidden task/channel/executor ownership; callbacks
must borrow that exact state mutably. Transitive calls, indirect targets and
state destructors need worker-safe effects. Movable opaque fields need an affine
owner, not a sharing promise, because callbacks serialize access. Fixed-array
elements participate in these checks, including at length zero.

Provider-context traversal additionally rejects reachable actor execution and
abandonment. Its cache is separate from ordinary worker certification, so a
previous worker check cannot bypass that restriction. This is not a non-trapping
contract: callback traps remain process-fatal.

These checks do not certify the callback ABI, returned block representation or
escaping storage lifetimes. There is no compiled callback adapter or public
retained handle yet. Initial language integration will exclude actor/arena
interoperability rather than imply that worker-safe fields survive abandonment.
Registration checks alone do not exclude later use of a handle in those contexts
or prove where pre-existing state was allocated; those remain bridge gates.

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

Registration rejects an active actor allocation target, actor recovery frame or
ambient arena with `CreateError::UnsupportedContext`. Rejection happens before
bootstrap allocation or taking ownership of state, and returns the original
provider. Bootstrap failures instead return `CreateError::Allocation`. Temporarily
routing an actor helper to System does not remove its recovery-frame restriction.
Neither rejection silently substitutes another allocator.

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

This is a Rust-internal interface, not a raw callback ABI for packages. No
compiled callbacks are connected yet; `Send` alone does not certify their
state, effects or destruction. Experimental non-atomic backing blocks remain
thread-confined.

### Callback trap boundary

Allocation, release and final provider destruction run inside an allocation-free,
thread-local scope. Traps in that scope are **process-fatal**, including inside
an actor. They report the original error and a callback-boundary note instead
of recovering through held mutexes or partially moved ownership. Invalid returned
blocks, recursive entry and poisoned-provider diagnostics follow the same rule.

Arena allocation-failure recovery is also disabled for the scope, including
arenas entered by a callback. The allocation-failure handler reports and aborts;
ordinary runtime traps report and exit with status 1. The boundary note uses
allocation-free, lock-free stderr writes so it is safe on the failure path.

This does not turn returned `AllocError` values into traps: they return normally.
Nested callbacks preserve the boundary, other threads do not inherit it, and
normal return or native Rust unwinding restores the prior scope. Rust panic
unwinding is tested only with Rust providers; this is not an unwinding ABI for
compiled Plenty callbacks. After the scope, ordinary actor/arena recovery works
as before.

The scope does not change allocation routing, certify escaping state or prove
callback effects safe. In particular, it does not make actor/arena-owned values
safe to retain past their backing lifetime. Those checks still belong to the
compiler bridge.

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

Callback-boundary checks use real actor/arena recovery frames in eleven bounded
subprocess cases: allocation, release and destruction traps; invalid extents;
recursive entry; poisoned locks; the allocation-failure handler under outer,
nested and actor-wrapped arenas; and successful/error returns followed by normal
recovery. A separate test checks allocation-free nesting, thread isolation and
scope restoration after native Rust panics. These are runtime probes, not
compiled-provider tests.

Nine compiler admission tests cover state/receiver checks, transitive and indirect
effects, destruction, actor restrictions, reachability and array traversal.
Two compiled-program regression tests cover fixed-array opaque contracts and
uncertified destructors; a valid movable-owner array releases both elements on a
worker. A bounded native subprocess verifies registration rejection under an
actor allocation target, actor recovery (including temporary System routing),
and an arena, with unchanged provider state and no bootstrap allocation. It also
checks thread isolation and successful registration/destruction after leaving
those contexts. This remains groundwork, not compiled-provider execution.

The 31-test allocator/registration harness passes Valgrind; its exec'd probes
are not child-traced. Directly tracing the registration probe fails during actor
heap creation, whose native path reserves 64 GiB of address space, before the
actor admission checks run. Those checks are validated natively, not under
Valgrind.

These are Linux x86-64 correctness results, not cross-platform qualification or
performance measurements. The metadata and clearing costs are deliberately not
optimized. Implementation priorities remain in
[B13](../../../backlog.md#b13-explicit-allocator-implementation-sequence).
