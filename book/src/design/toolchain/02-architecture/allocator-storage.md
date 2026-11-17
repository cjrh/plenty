# Retained allocator storage

`plenty-runtime/src/allocator.rs` implements the native storage substrate for
B13's builtin-owner integration. The [first public language binding](../../stdlib/alloc.md)
exposes the system allocator root for builtin `list[T]` construction and growth.
Also public are validated layout requests and affine safe byte blocks (`ByteBlock`)
with checked access, disjoint splitting and no copying or raw-pointer escape.
Compiled programs still select their existing allocation paths for all other
operations, and the experimental Plenty module remains unchanged for arena and
provider experiments. Native constructors now exercise selected-provider storage
for ordinary box/list representations; this is not yet completion of language-visible
builtin allocator support or custom-provider admission.

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

Two private `ThreadOp` markers track retained-allocator use and provider state:

- **`ThreadOp::AllocatorUse`** marks any retained-allocator operation, including
  explicit system-allocator storage without custom provider registration. It emits
  no code; every private `std.alloc` intrinsic produces it.
- **`ThreadOp::AllocatorProvider`** names provider state and its allocation/release
  functions. It emits no code and has **no frontend producer**. It reuses worker
  certification without requiring a worker spawn: state must be heap-storable and
  contain no hidden task/channel/executor ownership; callbacks must borrow that
  exact state mutably. Transitive calls, indirect targets and state destructors
  need worker-safe effects. Movable opaque fields need an affine owner, not a
  sharing promise, because callbacks serialize access. Fixed-array elements
  participate in these checks, including at length zero.

**Whole-program scope.** `check_with_exports` scans the complete lowered program,
including every unused and unrelated function body, before the no-worker/root
early return. This conservative scope ensures that retained allocator markers
combined with actor operations (`ThreadOp::Run`/`Forget` or
`CollectionOp::ActorHeap`) are rejected uniformly, even if those operations are
not immediately reachable from an entry point. The scan includes observation
heap helpers and any allocator use, whether through provider registration or
explicit system allocation. This is not a per-operation or per-context analysis:
until allocator provenance is tracked separately, any retained allocator and any
actor use in the same program are conservatively excluded.

**Single unified diagnostic.** Programs combining either marker with actor
execution, abandonment or heap operations fail with: "retained allocator use
cannot be combined with actor execution, abandonment or heap operations in the
same program; initial allocator integration excludes actor interoperability".
This is not a security boundary, capability tracking or proof of where
pre-existing state was allocated; those remain bridge gates during public
bindings and host-entry contracts.

These checks do not certify the callback ABI, returned block representation or
escaping storage lifetimes. There is no compiled custom-provider callback
adapter. Public system handles, lists and blocks exclude actor/arena
interoperability rather than imply that worker-safe fields survive abandonment.
Native registration guards remain in place, and `std.alloc` binding entries reject
ambient actor/arena contexts before allocation or ownership transfer. Trusted
foreign hosts must keep retained owners out of actor/arena invocations; the
program-wide check does not certify separately compiled foreign code.

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
thread-local scope. When a callback is invoked, managed destructors from the
runtime's iterative drop queue are suspended; the callback and any nested
destructors it triggers complete synchronously within the fatal scope before the
scope ends and outer pending drops resume. This nested-drop isolation ensures that
transitive cleanup of managed values stays within the callback's lifetime while
its provider remains alive, preventing callbacks from queuing managed cleanup
after their scope/lock ends.

Traps in that scope are **process-fatal**, including inside an actor. They report
the original error and a callback-boundary note instead of recovering through
held mutexes or partially moved ownership. Invalid returned blocks, recursive
entry and poisoned-provider diagnostics follow the same rule.

Arena allocation-failure recovery is also disabled for the scope, including
arenas entered by a callback. The allocation-failure handler reports and aborts;
ordinary runtime traps report and exit with status 1. The boundary note uses
allocation-free, lock-free stderr writes so it is safe on the failure path.

This does not turn returned `AllocError` values into traps: they return normally.
Nested callbacks preserve the boundary and nested-drop isolation; outer pending
drops are restored on success or native Rust unwinding. Other threads do not
inherit the scope. Rust panic unwinding is tested only with Rust providers; this
is not an unwinding ABI for compiled Plenty callbacks. After the scope, ordinary
actor/arena recovery works as before.

The scope does not change allocation routing, certify escaping state or prove
callback effects safe. In particular, it does not make actor/arena-owned values
safe to retain past their backing lifetime. Those checks still belong to the
compiler bridge.

## Private ByteBlock intrinsic ABI

Safe byte blocks are exposed through the public `Allocator.block()` method and
private intrinsic opcodes 650–656. Its private `array[u64, 3]` stores the backing
pointer, data pointer and byte extent, with eight bytes of inline tail padding.
The binding reads/writes these fields explicitly, never by casting Rust's
unspecified `Block` layout. Constructor and split operations write into
frame-owned buffers, then normal inline moves snapshot the result before those
buffers are reused. Descriptor moves do not retain backing; splitting does.
Final release uses the existing provider callback and managed-cleanup boundaries.

Every retained block intrinsic emits `AllocatorUse` and rejects ambient
actor/arena contexts before inspecting operands. Pure Plenty `layout()` validation
allocates nothing and needs no marker, but importing `std.alloc` still lowers
other marked bodies, so using only layouts does not bypass actor exclusion.
Private fields prevent direct construction, extraction or mutation of either
representation. `Allocator.block()` obtains fresh backing; `split_off()` creates
a disjoint owner of existing backing.

## Native builtin owner paths

`aggregates/allocator_owners.rs` constructs ordinary plain boxes and dense lists
using a selected allocator. Both the list object and its row buffer use that
provider. Empty lists retain it too. Compiler lowering reaches the list path
through `std.alloc`; selected-provider box construction remains native-only.

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

Compiled block-binding tests cover layout validation and privacy, zeroing and
alignment, moves/returns/container relocation, worker transfers and shared reads,
bounds traps, and allocation-free splitting under injected failures. Separate
native binding tests check custom-provider error preservation and final backing
release/provider destruction on a worker. These do not exercise compiled custom
providers, which remain unavailable.

Eleven additional native owner tests cover object/buffer routing, failed growth
and partial construction, instrumented system failures, nested records with
ranges and owned child boxes, extracted-value cleanup, dense scalar ABI offsets,
empty owners, take, final release on a worker and child-before-parent release.
A 10,000-node selected box graph drops on a 128 KiB stack. The combined allocator
checks pass Valgrind, including the new owner paths.

Callback-boundary checks use real actor/arena recovery frames in fourteen bounded
subprocess cases: allocation, release and destruction traps, including transitive
managed cleanup during an existing destruction pass; invalid extents; recursive
entry; poisoned locks; the allocation-failure handler under outer, nested and
actor-wrapped arenas; and successful/error returns followed by normal recovery.
A separate scope test checks allocation-free nesting, thread isolation and native
Rust panic restoration. Three managed-cleanup tests cover all callback kinds,
child-before-parent ordering, outer queue isolation and native unwind restoration;
cleanup before a recoverable allocation-error return; and 10,000 nodes on a
128 KiB stack. These are runtime probes, not compiled-provider tests.

Fourteen compiler admission tests cover provider state/receiver checks, transitive
and indirect effects, destruction, actor restrictions, reachability and array
traversal. Five new tests validate the program-wide exclusion of combined retained-allocator
and actor use: system-only use excludes actors without any certification
roots; unrelated and unused function bodies are conservatively scanned; nested
bodies cannot hide either exclusion half; feature-alone compatibility keeps
existing checks; and thread-safe exports do not relax the program-wide exclusion.
Two compiled-program regression tests cover fixed-array opaque contracts and
uncertified destructors; a valid movable-owner array releases both elements on a
worker. A bounded native subprocess verifies registration rejection under an
actor allocation target, actor recovery (including temporary System routing),
and an arena, with unchanged provider state and no bootstrap allocation. It also
checks thread isolation and successful registration/destruction after leaving
those contexts. System-handle and all seven block-binding opcodes are rejected
under actor recovery even inside temporary System routing, before reading block
operands. This remains groundwork, not compiled-provider execution.

The earlier 34-test allocator/registration harness passed Valgrind; its exec'd probes
are not child-traced. Directly tracing the registration probe fails during actor
heap creation, whose native path reserves 64 GiB of address space, before the
actor admission checks run. Those checks are validated natively, not under
Valgrind.

These are Linux x86-64 correctness results, not cross-platform qualification or
performance measurements. The metadata and clearing costs are deliberately not
optimized. Implementation priorities remain in
[B13](../../../backlog.md#b13-explicit-allocator-implementation-sequence).
