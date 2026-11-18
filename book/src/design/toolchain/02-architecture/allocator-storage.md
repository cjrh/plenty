# Retained allocator storage

`plenty-runtime/src/allocator.rs` implements B13's native storage substrate.
The [public `std.alloc` binding](../../stdlib/alloc.md) supports system roots,
compiled custom-provider registration, safe byte blocks and selected builtin
boxes, lists (including generator collection), dictionaries and sets. Callers
can also select storage for [independent set results](#independent-set-results),
[copies](#selected-copy) and [owning list slices](#selected-list-slices). Providers receive validated `Layout` requests
and return affine `ByteBlock` owners, without raw-pointer escape.
`Allocator.box` calls the private intrinsic `__allocator_box` (opcode 658) with a
borrowed source value. The std wrapper then commits that source with
`__allocator_commit`, which consumes it without running its destructor, and only
after a successful box. The generator collector is ordinary Plenty iteration over
the selected list path, so it adds no collector intrinsic. Other allocating
language/std operations still use their existing paths. The experimental module
remains separate.

## Thread contract decision

Builtin allocator handles support safe cross-thread use from the outset,
without allocator type parameters on builtin owners. Worker checks still inspect
their contents; hiding a thread-confined provider inside an otherwise
worker-eligible collection would bypass those checks.

The native substrate therefore retains only `Send + 'static` Rust providers and
serializes their allocation and release callbacks. Compiled Plenty providers
also pass compiler certification of their state, effects and destruction.
Thread-confined Plenty providers remain supported by the
[experimental generic owner](../../stdlib/experimental-owned-list.md).

## Compiler admission

Two private `ThreadOp` markers track retained-allocator use and provider state:

- **`ThreadOp::AllocatorUse`** marks any retained-allocator operation, including
  explicit system-allocator storage without custom provider registration. It emits
  no code; every private `std.alloc` intrinsic produces it.
- **`ThreadOp::AllocatorProvider`** names provider state and its allocation/release
  functions. Private registration lowering in `frontend/allocator_storage.rs`
  emits it and specializes the std callback wrappers. It reuses worker
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
pre-existing state was allocated; trusted host-entry contracts must still
exclude incompatible provenance.

These checks are paired with private compiled callback adapters and runtime
block validation; they are not arbitrary foreign-provenance certification.
Public handles, providers and selected owners exclude actor/arena
interoperability rather than imply that worker-safe fields survive abandonment.
Native registration guards remain in place, and `std.alloc` binding entries reject
ambient actor/arena contexts before allocation or ownership transfer. Trusted
foreign hosts must keep retained owners out of actor/arena invocations; the
program-wide check does not certify separately compiled foreign code. Unused
imports participate, including programs that use only `Layout`.

## Explicit selection and bootstrap

An `Allocator` handle contains either a retained provider-state pointer or the
explicitly selected system root. Obtaining or cloning the system root allocates
nothing. There is no current/default allocator and no fallback after provider
failure.

Native provider registration requires an explicit bootstrap allocator for its
aligned control/state allocation, including reference count, callback table,
mutex and provider payload. Compiled registration in `allocator/compiled.rs`
makes exactly two allocations through the explicit bootstrap: a selected box
holding the Plenty `P`, then this control allocation holding its adapter. There
is no hidden `Arc`, system allocation or fallback. Existing allocations nested
in `P` keep their original provenance.

The private source is borrowed during registration. First-allocation failure
leaves it untouched; second-allocation failure releases the provisional state
box without dropping the relocated copy of `P`. Either failure returns the
original provider unchanged. Success is followed immediately by the std
wrapper's non-dropping source commit: no fallible operation or user callback may
intervene. This transfer avoids both duplicate destruction and retaining a
second copy of managed fields. Successful owners retain the provider and
bootstrap backing through final state and child destruction.

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

The bridge connects only compiler-certified callbacks; it is not a public raw
callback ABI for packages. `Send` alone would not certify compiled state,
effects or destruction. Experimental non-atomic backing blocks remain
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
compiler admission and trusted host contracts.

## Compiled callback ABI

`codegen/allocator_providers.rs` emits local C scalar/pointer adapters, not a
Rust or Plenty aggregate/enum ABI. Allocation receives the state-box pointer,
size, alignment and caller-owned output storage, returning a scalar status.
Release receives the state-box pointer and block-storage pointer. The adapters
borrow the exact `P` slot and call specialized private std wrappers.

Block transfer uses three explicit words (backing pointer, data pointer, extent)
with the inline ABI's tail padding, not Rust's unspecified `Block` layout.
Allocation snapshots the returned descriptor into caller-owned storage before
its frame expires; release snapshots incoming storage before the owned Plenty
wrapper consumes it. Native extent/alignment validation precedes typed writes.
Final release returns the full original block, zeroed throughout, not an empty
range. Both adapters run while the provider mutex is held and the fatal-cleanup
scope is active. Final state destruction runs in that scope after the last
retained reference is gone; no concurrent callback remains to require a lock.

A std-only consuming intrinsic extracts ByteBlock storage without running its
destructor. Ordinary field projection would destroy the transferred backing.
The private registration commit likewise consumes the original provider without
cleanup; neither operation exposes a general-purpose forget facility.

## Private ByteBlock intrinsic ABI

Safe byte blocks are exposed through the public `Allocator.block()` method and
private intrinsic opcodes 650–656. Its private `array[u64, 3]` stores the backing
pointer, data pointer and byte extent, with eight bytes of inline tail padding.
The binding reads/writes these fields explicitly, never by casting Rust's
unspecified `Block` layout. Constructor and split operations write into
frame-owned buffers, then normal inline moves snapshot the result before those
buffers are reused. Descriptor moves do not retain backing; splitting does.
Final release uses the existing provider callback and managed-cleanup boundaries.

Every runtime block-binding intrinsic emits `AllocatorUse` and rejects ambient
actor/arena contexts before inspecting operands. Pure Plenty `layout()` validation
allocates nothing and needs no marker, but importing `std.alloc` still lowers
other marked bodies, so using only layouts does not bypass actor exclusion.
Private fields prevent direct construction, extraction or mutation of either
representation. `Allocator.block()` obtains fresh backing; `split_off()` creates
a disjoint owner of existing backing.

## Native hash-table storage

The four public [dictionary and set constructors](../../stdlib/alloc.md#dictionaries-and-sets)
share one std-only intrinsic, `__allocator_dict_set` (opcode 659). Lowering passes
the complete builtin collection descriptor, allocator handle and requested entry
count. The hash path builds ordinary dictionaries and sets whose
object, bookkeeping, rows, insertion-order links and bucket table use the
selected allocator. Empty owners retain the allocator before the first insert.
Rehashing builds the replacement table through the same provider. If growth
fails, lookups, contents, order and live slot IDs survive; a successful link
reservation need not surrender its capacity if a later row reservation fails.

Row relocation repairs inline values in live slots, skipping holes. Destruction
finishes key and value cleanup before releasing the object's backing. Selecting
the table does not redirect allocations inside its keys or values, including a
new text key built from bytes. Copies and set algebra produce independent owners
and are outside this construction path.

The binding rejects negative capacity before calling a provider. For a compact
finite set, reservation checks the ordinary entry-count overflow limit, not
whether the hint fits the domain. The selected object itself holds the inline
membership mask; valid hints above the domain need neither row nor bucket
storage. Member insertion uses that mask, with the same canonical slots as an
ordinary bitset. Only the owner block needs release.

## Independent set results

The four public [`Allocator` algebra methods](../../stdlib/alloc.md#set-algebra-with-an-output-allocator)
declare two `&set[K]` operands. For a direct call involving a `KeyView[K]`,
the frontend specializes the trusted `std/alloc.plentyi` wrapper with the
actual operand types. It tracks each view back to its dictionary, keeping the
first source borrowed while evaluating the second. No view-to-set conversion
or public overload participates in generic or indirect calls.

The std-only intrinsics (opcodes 660–663) pass the result descriptor `set[K]`
separately: neither operand's descriptor is necessarily a set. After the
actor/arena guard, `try_set_algebra_in` calls the same membership algorithm as
ordinary algebra, with the receiver's allocator as its destination. Two bitsets
combine their masks directly; otherwise the algorithm counts result members
and reserves once before copying selected keys. It accesses dictionary keys in
place, never reading the value field or staging the input keys in a set. A
finite-domain bitset result uses a single selected owner even when an input is a dictionary.

The recursive copy routine uses the selected allocator whenever it allocates a
new copy. Existing shared leaves, such as immutable text, retain their backing.
This does not widen `KeyType`: legal affine keys that can be explicitly copied
work, but a key with custom cleanup anywhere in its type cannot be duplicated.
If allocation fails, cleanup releases only partial output and the caller gets
the allocation error with both inputs unchanged.

[`Allocator.to_set`](../../stdlib/alloc.md#copy-dictionary-keys-into-a-set) takes
one key view and copies all its keys, without a membership comparison. Only
this trusted `std/alloc.plentyi` declaration takes a `KeyView[K]` parameter.
It tracks the argument's loan back to the dictionary. Worker eligibility is
checked on that dictionary, not inferred from `K`: a view may be created
inside a worker, but cannot enter as a job argument or capture.

`ALLOCATOR_KEY_VIEW_TO_SET` (665) checks the actor/arena guard before reading
the operand buffer. The compiler supplies a `set[K]` descriptor separately from
the borrowed dictionary. `try_key_view_to_set_in` passes both to `try_members_copy`
with the selected destination. The walk copies only keys, so neither construction
nor rollback needs to duplicate or destroy a dictionary value. Ordinary set
methods, including `view.to_set()`, do not select an allocator.

## Selected copy

`Allocator.copy` in `stdlib/std/alloc.plentyi` calls std-only `__allocator_copy`.
`src/frontend/retained_allocator.rs` rejects loan-carrying `T` (including a
class with a slice-view field), then applies the builtin's `check_explicit_copy`.
It emits `ThreadOp::AllocatorUse` and passes the type descriptor with
`ALLOCATOR_COPY` (opcode 664). Unlike selected boxing, there is no commit: the input stays
borrowed. `lower_c_intrinsic` in `src/codegen/foreign.rs` reads the typed
reference, packs the operand and supplies private frame storage for any inline
payload. The result must point into its own storage, not into the input record.

The native `operation` entry checks the actor/arena guard before reading the
operand buffer; `collection_at` borrows the guarded handle before inspecting
the descriptor. `try_copy` relocates the inline payload and calls `try_copy_into`
with `CopyDestination::Selected`. Ordinary `CollectionOp::TryCopy` (33) uses
the same walk with `Ordinary`. The [allocation-site inventory](allocation-sites.md)
records the destination and cleanup rules for its branches, and which key-view
and slice operations use different paths.

## Selected list slices

[`Allocator.slice`](../../stdlib/alloc.md#copy-a-list-slice-with-an-output-allocator)
uses the bracket slice element check, not the consuming list method. Lowering
rejects loan-carrying elements and passes the result descriptor `list[T]` to
`__allocator_list_slice`. The Plenty wrapper maps each `i64 | Unset` bound to
`Option[i64]` and unwraps the `NonZero[i64]` step. `ALLOCATOR_LIST_SLICE` (666)
checks the actor/arena guard before reading the operands. It derives bound
presence flags from the option tags, so every integer, including `-1` and
`i64.MIN`, remains an explicit bound.

`try_list_slice_stepped` normalizes the range once and builds the result
through `CopyDestination::Selected`. Ordinary brackets use the same row walk
with `Ordinary`. Neither path stages a list of indices or copies unselected
rows. See the [slice allocation inventory](allocation-sites.md#owning-slices-and-their-non-owning-lookalikes)
for nested copies, rollback and the separate temporary-transfer path.

## Native builtin owner paths

`aggregates/allocator_owners.rs` constructs ordinary plain boxes and dense lists
using a selected allocator. Both the list object and its row buffer use that
provider. Empty lists retain it too. Selected boxes also hold compiled provider
state. Compiler lowering reaches the box
path through `Allocator.box` (opcode 658) and the list path through
`Allocator.empty_list` (opcode 616); the collector then grows that list with
ordinary `append`.

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

Constructing a selected owner does not redirect subsequent builtin `copy`,
bracket slices, stable-sort scratch or adapted protocol/callable boxes. Its
retained allocator is for its own storage growth and destruction.

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
release/provider destruction on a worker. Compiled custom providers have separate
coverage in `tests/test_std_alloc_provider.rs`.

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
operands. These admission checks complement compiled-provider execution.

The earlier 34-test allocator/registration harness passed Valgrind; its exec'd probes
are not child-traced. Directly tracing the registration probe fails during actor
heap creation, whose native path reserves 64 GiB of address space, before the
actor admission checks run. Those checks are validated natively, not under
Valgrind.

### Compiled-provider coverage

`tests/test_std_alloc_provider.rs` covers state retention through lists/blocks,
both bootstrap failure points with unchanged returned providers, independent
budgets without fallback, worker callbacks and final managed destruction,
serialized mutation, malformed extent/alignment, and process-fatal traps in
callbacks and transitive cleanup. Admission cases include public mutable method
signatures, borrowed layouts, hidden channel ownership, transitive effects,
opaque state and host-callback restrictions without a spawn. Inline generic,
copyable and enum state exercise the private transfer ABI. A `runtime-checks`
case injects actual allocation failures to check the two-allocation registration
budget. The [delivery record](../../../completed-work/language-and-tooling.md#public-compiled-custom-providers)
and local PR distinguish executed checks from coverage that is only present in
source, including native-only trap probes and directly traced Valgrind runs.

`tests/test_std_alloc_consumers.rs` covers selected box ownership, failure and
returned values, take and worker rules, collector failure cleanup and capture
loans, and selected-list result inference. `tests/test_std_alloc_dict_set.rs`
covers structural keys, selected hash growth and cleanup on a worker, capacity
and provider errors (including finite sets), and ordinary admission restrictions.
`tests/test_std_alloc_set_algebra.rs` exercises the public wrappers on hash sets
and bitsets, set/view and view/view calls, provider failure, result lifetime
and growth, scoped workers, and rejection of nonduplicable keys and actor use.
These are Linux x86-64 correctness results, not cross-platform qualification or
performance measurements. The metadata and clearing costs are deliberately not
optimized. Implementation priorities remain in
[B13](../../../backlog.md#b13-explicit-allocator-implementation-sequence).
