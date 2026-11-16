# Explicit allocators: narrow experiments

**Status: experimental implementation and measured correctness evidence for B13.**
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
- Retained allocator state and scoped/resettable arenas are separate designs.
  This experiment implements retained state with no arena reset.

## What was built

[`std.experimental.alloc`](../design/stdlib/experimental-alloc.md) implements
system handles, bounded bump arenas and `IntList` in Plenty. Private trusted
raw access supplies scalar loads/stores and C `malloc`/`free`. Compiler changes
only register this module as a trusted std interface; there are no new parser,
collection-lowering or runtime allocation operations.

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

## What this does not establish

This is a two-policy class, not a user-implementable allocator protocol or vtable.
The raw interface supports only scalar slots, not a generic layout/drop ABI.
It does not prove generic collections, nested owned payload cleanup, strings,
formatting, boxed protocols, thread eligibility, resettable arena lifetimes,
actor integration or the complete std migration. Its C allocator bypasses the
runtime's small-block pool and ordinary allocation-failure accounting.

These are prototype boundaries, not arguments for retaining implicit allocation
in the final language. The experiment does not change existing program behavior
or advertise B13 as complete.
