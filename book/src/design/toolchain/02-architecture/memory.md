# Memory and destruction

Each live managed operand, local, field, and capture has an owner. Moving an affine
value clears its source slot. Loading a copyable inline value copies its storage
and retains contained strings. Stores stage the new value before releasing the
old owner. Source cleanup order is defined by
[destruction](../../language/05-ownership/06-destruction.md), not reference counts.

Runtime objects share `{atomic u64 refs, destroy_callback}`. Heap objects begin
with one reference; literals use an immortal count. A
[`Shared[T]`](../../facilities/01-builtins/shared.md) handle is a box whose
count is shared by its source-level owners. Native helpers borrow inputs
and return owned managed results, including retained projections and builder aliases.
Buffers have explicit owners; type metadata is immutable program data.

Retain uses relaxed atomic updates. Final release acquires preceding releases
before destruction; the final thread owns the destruction queue entry. This
protects ownership counts, not concurrent mutable payload access or foreign
resource transfer. Source permissions still come from borrow and worker checks.

An iterative queue destroys owned graphs without recursive native-stack growth.
Inline generator cleanup uses the count handoff but finishes synchronously while
its enclosing storage is live. Private helper aliases do not authorize mutable
source aliases. Traps end the process without unwinding language scopes.

## Allocator provenance

Object allocations carry a private prefix identifying their allocator, leaving
generated-code header and payload offsets unchanged. Freeing uses that identity,
never a later global choice. Internal allocator tables must live for the process.
Ordinary objects select the global allocator; `Vec`-backed container buffers use
it directly. Public allocator selection and scoped allocator lifetimes are unsupported.

Checked buffer growth and partial-construction cleanup implement the
[recoverable allocation contract](../../language/08-errors/02-allocation-and-traps.md).
The `runtime-checks` feature counts allocation attempts, including allocations
freed inside the observed region, for native integration tests.
