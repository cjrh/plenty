# Sets

Dictionary [`KeyView[K]`](dictionaries.md#snapshots-and-views) values borrow
their dictionary's keys without allocating and provide the same relationship
and algebra methods. Their operations accept `&set[K]` or `&KeyView[K]`;
equality also compares a key view and a set in either operand order.

Sets use the [common collection rules](collections.md); members are integers,
booleans, strings, `bytes`, or tuples, classes and named enums recursively containing those types, without custom cleanup.
Iteration order is unspecified. `add` and `reserve` follow
the [recoverable growth contract](construction-copy.md).

## Removal and update

`set.discard(value) -> bool` requires exclusive access. It observes one
same-typed value or reference before borrowing the receiver, removes and releases
the stored member if present, and returns whether removal occurred. A miss changes
nothing. It allocates no storage and retains capacity. Set `pop` is unsupported.

`set.update(other) -> Result[(), AllocError]` consumes one same-typed owned set
on both outcomes. It reserves for absent members before changing membership.
Failure preserves destination contents and lookup behavior and drops the source;
success transfers new owners and releases duplicate incoming owners. Empty and
duplicate-only sources, or sufficient reserved capacity, need no allocation.
Use `copy(source)?` explicitly to retain a source. References and general or
multiple iterables are unsupported.

`set.intersection_update(other) -> ()` keeps common members;
`set.difference_update(other) -> ()` removes members found in other.
Both accept a same-key-type dictionary `KeyView` as well as a set. They
require exclusive destination access and observe the source before
borrowing the destination. The source remains borrowed during mutation; aliases
into the destination are rejected. Removed members drop, capacity remains, and
the operations allocate nothing. Use `clear()` to empty a set directly.

## Set algebra

Each method takes exactly one same-key-type set or dictionary `KeyView`
(with an optional explicit borrow), observes both inputs,
and returns `Result[set[T], AllocError]`:

| Operation | Members selected |
| --- | --- |
| `set.union(other)` | Present in either input |
| `set.intersection(other)` | Present in both |
| `set.difference(other)` | Present in the receiver, absent from other |
| `set.symmetric_difference(other)` | Present in exactly one input |

Inputs may be the same set and remain unchanged on failure. Output is independent,
though immutable strings can share storage. All storage for the actual result is
reserved before copying members, including move-only class records. Hash-backed
results use four allocations when nonempty, one for an empty owner. Small enum
bitsets use one owner allocation regardless of membership. Arithmetic operator overloads and iterable conversions
are not implied.

## Relationships

`issubset(other)`, `issuperset(other)`, and `isdisjoint(other)` return `bool`.
They take one same-key-type set or dictionary `KeyView`, accept references,
observe both operands once
left to right, and allocate nothing. Empty sets are subsets of every set and
disjoint from every set. Heterogeneous coercion and comparison operators are
unsupported.

## Small enum bitsets

A set whose element type is a payload-free named enum with at most 64 variants
uses a 64-bit membership mask instead of rows and hash buckets. This is automatic:
write `set[Permission]`, with the same methods and Result handling as any set.
The owner still allocates once; construction, copying and algebra can therefore
return `AllocError`. Adding members, valid reservation, removal and in-place
updates need no further allocations. Invalid capacity requests still return
`CapacityOverflow`.

Membership, insertion, removal, length and relationships between two such sets
use bit operations. Their union, intersection, difference and symmetric
difference use word operations plus the result-owner allocation. Iteration visits
present variants; borrowed iteration still yields shared references, and the
borrow checker still protects the source owner. Set iteration order remains
unspecified; do not rely on the order seen with a particular representation.

Dictionary key views remain borrowed hash-table views. Their mixed operations
with bitsets work normally, and `to_set()` can produce a bitset. Dictionary
storage itself is unchanged. Enums with payloads, enums with more than 64
variants, and classes keep the ordinary hash-set representation, even when their
domain might be finite. Wider bitsets and finite-payload encoding are separate
extensions; no SIMD support is required by the 64-bit implementation.
