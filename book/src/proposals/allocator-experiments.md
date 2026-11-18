# Explicit allocators: narrow experiments

**Status: experiment history and delivered `std.alloc` provider and builtin-owner support for B13.**
This revises the default-allocator direction in the
[earlier memory proposal](memory-parallelism-and-simd.md#custom-allocators-without-multiplying-everyday-types).
It does not migrate existing allocation sites. Outstanding work remains in
[B13](../backlog.md).

## Agreed direction

- Allocating Plenty mechanics and APIs specify an allocator. A system allocator
  is an ordinary obtainable value, not an implicit fallback.
- Multiple allocators coexist. Containers retain their selected allocator for
  growth and automatic destruction.
- Package functions can accept allocators; this is not a security boundary.
  Code can explicitly obtain a system allocator or call a foreign allocator.
- Prefer an explicit collector such as `allocator.list(a for a in b)?` over
  adding allocator-specific comprehension syntax.
- The eventual migration must cover strings, boxes, formatting, copying, owning
  slices, scratch buffers and std APIs, not just collection construction.
- User-defined allocation policies should first use safe owned backing blocks,
  with raw pointer validity and typed-value cleanup inside the trusted boundary.
  This does not make arbitrary raw allocator callbacks a safe public interface.
- Retained allocator state and scoped/resettable arenas are separate designs.
  This experiment implements retained state with no arena reset.

## Builtin thread-contract decision

Builtin handles will be thread-safe from the outset, keeping ordinary
`list[T]` and `Box[T]` types without allocator type parameters. Existing worker
checks inspect their elements, so simply erasing a thread-confined provider
would be unsound. The selected approach serializes provider callbacks and
requires worker-safe state, effects and destruction; the rejected alternative
was an additional statically thread-confined builtin owner form.

The [native storage substrate](../design/toolchain/02-architecture/allocator-storage.md)
implements retained state and serialized Rust callbacks, with explicit bootstrap
allocation. Public `std.alloc` now also registers compiled providers with
worker-certified state, callback effects and destructors. This preserves ordinary
builtin owner types without erasing a thread-confined policy into them.
Thread-confined providers remain useful with the experimental generic owner.
Scoped reset is still a separate lifetime contract.

Provider callback traps are process-fatal, even inside actors. Returning an
allocation error remains recoverable; a trap is not an allocation-error return.
The native boundary disables actor and arena nonlocal recovery during allocation,
release and provider destruction, so neither can abandon held locks or partially
moved ownership. Preserving actor-local callback trap recovery would require a
separate isolation/cleanup design; requiring statically non-trapping callbacks
would instead need a restrictive transitive effect contract. Neither alternative
is selected.

## Temporary program-wide allocator/actor exclusion

Initial public retained-provider integration excludes actor/arena
interoperability rather than implement abandonment cleanup now. Thread-safe
fields alone do not prove their backing outlives recovery or reclamation.
Registration checks also cannot detect a handle created earlier and later
carried into an actor through an ordinary collection.

The selected initial restriction is therefore program-wide, not targeted
provenance tracking. Private `ThreadOp::AllocatorUse` and
`ThreadOp::AllocatorProvider` markers cannot coexist with actor execution,
abandonment or heap operations in one lowered program. This includes unused and
unrelated bodies and heap observation helpers. System-only retained allocation
participates too: private `std.alloc` storage intrinsics, including dictionary
and set construction and selected set algebra, emit `AllocatorUse`. Registration lowering emits the private
custom-provider marker. Unused imports and layout-only use do not bypass this
gate. Existing allocation syntax is unchanged.

Native registration rejects active actor/arena contexts without bootstrap
allocation and returns the provider unchanged. The compiler gate does not replace
runtime guards at bindings or foreign host-entry contracts, or prove arbitrary
pre-existing state provenance and escaping backing lifetimes. There is no silent
system-allocation substitute for rejected contexts. Remaining implementation belongs to
[B13](../backlog.md#b13-explicit-allocator-implementation-sequence).

## What was built

A [public binding](../design/stdlib/alloc.md) for `std.alloc` now delivers:

- `layout(size, alignment) -> Result[Layout, LayoutError]` — validated,
  allocation-free layout requests with scalar alignment checking and capacity
  validation.
- `Allocator.block(layout) -> Result[ByteBlock, AllocError]` — zero-initialized
  affine byte blocks with checked access, disjoint splitting and no copying.
- `system()` and selected builtin boxes and lists, including generator collection.
- [Empty and capacity constructors for dictionaries and sets](../design/stdlib/alloc.md#dictionaries-and-sets),
  with retained hash storage or the ordinary inline finite-domain bitset.
- [Set algebra on borrowed sets or dictionary key views](../design/stdlib/alloc.md#set-algebra-with-an-output-allocator),
  with an explicit output allocator rather than a choice inferred from either input.
- [Copy with an output allocator](../design/stdlib/alloc.md#copy-with-an-output-allocator),
  borrowing the source and retaining existing shared leaves.
- `allocator(bootstrap, provider)` — compiled custom-provider registration with
  an explicit bootstrap and `RegistrationError[P]` returning `P` on failure.
  `Provider.allocate` borrows a validated `Layout`; `release` consumes the full
  original backing, zeroed before safe byte access.

The bridge makes two bootstrap allocations (selected state box and control),
with no hidden fallback or migration of allocations already nested in `P`.
Either allocation failure returns `P` unchanged; success retains state and
bootstrap storage through owner cleanup. Private scalar/pointer C adapters
connect certified callbacks, and runtime validation checks returned extent and
alignment before typed writes. Traps, including transitive cleanup and final
state destruction, are process-fatal; returned `AllocError` remains recoverable.
This is a public compiled-provider milestone, not the language/std migration.
The [reference](../design/stdlib/alloc.md#custom-providers) owns its contract and
[architecture](../design/toolchain/02-architecture/allocator-storage.md#compiled-provider-coverage)
names its test coverage.

### Separate experimental owners

The static-provider generic owner remains experimental. [`std.experimental.alloc`](../design/stdlib/experimental-alloc.md)
implements system handles, bounded bump arenas and `IntList` in Plenty. Private
trusted raw access supplies scalar loads/stores and C `malloc`/`free`. Compiler
changes for that initial experimental system/arena/IntList increment only
registered its trusted std interface; later increments added private typed-slot
and retained allocator operations.

The intentionally narrow list has inline ownership metadata and an allocated
`i64` buffer. Each list retains its allocator; an arena retains its parent.
The system handle allocates 64 bytes of bootstrap state. Arena construction
requests its 64-byte state and bounded backing block from the explicit parent.
No global/current allocator variable participates in those operations.

Runnable programs:

| Program | Observation |
| --- | --- |
| `examples/allocators/system.plenty` | Existing generator-as-argument syntax works; growth leaves one live buffer and dropping the list leaves zero. |
| `examples/allocators/arena.plenty` | Four-to-eight-slot growth consumes 32 + 64 = 96 arena bytes. A ninth insertion fails and preserves eight values. Releasing the list leaves zero live blocks but 96 consumed bytes. |
| `examples/allocators/mixed.plenty` | One helper accepts either allocator. Interleaved lists use distinct storage policies; explicit copy migrates values. Both lists grow after the caller drops its allocator handles. |

Run from the checkout with the compiler built on this branch:

```sh
cargo build --locked --bin plenty
target/debug/plenty examples/allocators/system.plenty
target/debug/plenty examples/allocators/arena.plenty
target/debug/plenty examples/allocators/mixed.plenty
```

## Validation and findings

`tests/test_allocator_probes.rs` has seven tests. Every successful program runs
with optimization off and on. The tests also cover independent arenas, a nested
arena escaping through its returned list, failure during collection and growth,
zero/tiny budgets, impossible capacities, partial arena construction, rejected
implicit copying/private raw access and rejected worker transfer.

A C linker wrapper fails actual `malloc` calls at selected counts: system
metadata, arena metadata, arena backing, partial list growth and mutation of an
existing list. The wrapper is armed only around the operation being tested.
This is independent of the module's own accounting counters.

Validation performed on Linux x86-64:

```sh
cargo test --locked --test integration test_allocator_probes::
PLENTY_TEST_VALGRIND=1 cargo test --locked --test integration test_allocator_probes::
PLENTY_TEST_VALGRIND=1 just test --filter test_allocator_probes::
```

All passed seven tests, including the all-features focused run through `just`. Valgrind covers both compilation modes, including
injected failures, and makes invalid access or definite/indirect/possible leaks
fail the test. Other targets and concurrent allocation are unqualified.

The useful conclusions are limited but concrete:

1. **Comprehension removal is not prerequisite work.** The selected replacement
   spelling already parses and executes as an ordinary method taking a generator.
2. **Allocator policy can live in Plenty.** A trusted storage boundary is still
   necessary; duplicating the whole policy implementation in Rust is not.
3. **Retained allocators compose.** Dropping an initiating handle does not
   invalidate its lists or child allocators. Nested arenas retain their backing
   provider, and final destruction follows that chain.
4. **Arena growth has a visible cost.** Copying to a larger buffer abandons the
   old space until final arena release. Reserving the expected final capacity
   avoids this; a later last-allocation resize optimization needs its own tests.
5. **Failure handling and allocator routing are independent.** Existing `Result`,
   `?` and destruction work for this library owner, but builtin buffers still
   need explicit routing changes.

## Safe external block-policy increment

The [safe backing-block API](../design/stdlib/experimental-alloc.md#safe-backing-blocks)
adds affine zero-initialized byte blocks, checked access and disjoint splitting.
`examples/allocators/block_provider.plenty` is an ordinary external bump provider;
`owned_blocks.plenty` consumes it through the generic `BlockProvider` protocol.
No new compiler operations were needed for this byte-only increment.

Tests cover independent budgets, retained backing after provider destruction,
split alignment, failure without budget mutation, invalid-access traps,
unforgeable/private ownership and prohibited copying/thread transfer. Actual
malloc injection covers metadata and backing failure. Successful programs run
in both compilation modes and under Valgrind. The byte-only increment was a
partial stage-1 result; its extension and remaining migration are recorded in
[B13](../backlog.md#b13-explicit-allocator-implementation-sequence).

## Generic owned-payload gate

[`OwnedList[T, P]`](../design/stdlib/experimental-owned-list.md) now stores generic
owned values in provider-selected blocks. Private compiler operations use the
existing slot layout and relocate nested inline payloads rather than copying
pointers into expired stack frames. Extent and alignment are validated before
access; initialized slots are moved out and cleared before releasing storage.
The provider is retained by value, receives emptied blocks through `release`,
and is destroyed only after element and buffer cleanup.

The allocator suite now has twenty tests. It covers nested records, arrays,
enums, strings, collections, custom destructors and unit; strong growth failure
with returned inputs; cleanup during error propagation; caching of released
blocks; rejected malformed providers, private raw access and hidden task/channel
ownership. Successful fixtures run optimized and unoptimized under Valgrind,
including injected actual-malloc failures. This satisfies the bounded stage-1
ownership gate without migrating builtin owners.

## Selected boxes and generator collection

The [public selected-box and collector increment](../design/stdlib/alloc.md#selected-boxes-and-collection)
adds two operations to the `std.alloc` bridge:

- `Allocator.box(value)` moves a value into an ordinary concrete `Box[T]` whose
  storage uses the selected allocator. Failure returns the unchanged value in
  `BoxError[T]`. The compiled path uses the private box intrinsic (opcode 658)
  with a borrowed source and a success-only commit.
- `Allocator.list(source)` collects a generator by ordinary Plenty iteration over
  a selected empty list and `append`. No collector intrinsic was added. The
  generator is consumed on every return; failures drop its captures, current item
  and partial output exactly once.

The collector has no item cap of its own. Callers can bound the source or select
a budgeted provider; the system root imposes no finite budget. Consuming failure
and the absence of a cap are narrow, adopted exceptions recorded in the [std conventions](../design/stdlib/conventions.md#returning-moved-inputs);
other std APIs keep the existing rules. The increment leaves actor exclusion,
callback safety and registration's two bootstrap allocations unchanged. Its
tests are in `tests/test_std_alloc_consumers.rs`.

## What this does not establish

The original experimental allocator remains a two-policy class; its generic
owner uses static provider dispatch, not the public retained-handle bridge.
Those experiments do not migrate builtin allocation or establish worker safety.
The separate public bridge supports blocks, boxes, lists (including generator
collection), dictionaries and sets, but not full language/std allocation routing,
removal of implicit defaults, resettable arenas or actor integration. The
experimental C allocator bypasses the
runtime's small-block pool and ordinary allocation-failure accounting.

These are prototype boundaries, not arguments for retaining implicit allocation
in the final language. The experiment does not change existing program behavior
or advertise B13 as complete.
