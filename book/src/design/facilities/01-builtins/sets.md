# Sets

Sets use the [common collection rules](collections.md); members are integers,
booleans, strings, `bytes`, or tuples recursively containing those types.
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
Both require exclusive destination access and observe a same-typed source before
borrowing the destination. The source remains borrowed during mutation; aliases
into the destination are rejected. Removed members drop, capacity remains, and
the operations allocate nothing. Use `clear()` to empty a set directly.

## Set algebra

Each method takes exactly one same-typed set or reference, observes both inputs,
and returns `Result[set[T], AllocError]`:

| Operation | Members selected |
| --- | --- |
| `set.union(other)` | Present in either input |
| `set.intersection(other)` | Present in both |
| `set.difference(other)` | Present in the receiver, absent from other |
| `set.symmetric_difference(other)` | Present in exactly one input |

Inputs may be the same set and remain unchanged on failure. Output is independent,
though immutable strings can share storage. All storage for the actual result is
reserved before retaining members: four allocations for a nonempty result,
one for an empty owner. Arithmetic operator overloads and iterable conversions
are not implied.

## Relationships

`issubset(other)`, `issuperset(other)`, and `isdisjoint(other)` return `bool`.
They take one same-typed set, accept references, observe both operands once
left to right, and allocate nothing. Empty sets are subsets of every set and
disjoint from every set. Heterogeneous coercion and comparison operators are
unsupported.
