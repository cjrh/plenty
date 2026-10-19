# Arena prototype assessment

This accompanies the [arena design assessment](../arenas.md). It is an experiment
in existing language facilities, not a stable standard-library contract.

Run the demo from the repository root:

```sh
cargo run --locked --bin plenty -- examples/arena/main.plenty
```

`examples/arena/arena.plenty` owns `list[Slot[T]]`; each slot stores an optional
value, a generation, and a free-list link. `Key[T]` carries the namespace, slot,
and generation, with no stored `T`. The demo transfers between two integers,
removes one, reuses its slot, and shows that the old handle fails lookup.
`examples/arena/parent_links.plenty` stores typed parent handles inside nodes,
duplicates a handle without copying its node, and detects a removed parent.

| Prototype operation | Result and behavior |
| --- | --- |
| `Arena[T](namespace, generation_limit=i64.MAX)` | Fallible construction; namespace uniqueness is the caller's responsibility. |
| `insert(value)` | `Result[Key[T], AllocError]`; reuse a vacant slot or grow storage. |
| `contains(key)` | Check namespace, bounds, generation, and occupancy. |
| `remove(key)` | `Option[T]`; transfer the owner and invalidate its handle. |
| `read(key, callback)` | `Option[R]`; callback receives `&T` and returns a storable result. |
| `update(key, callback)` | `Option[R]`; callback receives `&mut T`. |
| `update_two(a, b, callback)` | `Option[R]`; reject identical slots or invalid handles, then lend both values to a callback. |
| `len()` | Number of live values, excluding vacant or retired slots. |
| `key.duplicate()` | Another key for the same node, without allocation or payload copying. |

The module uses a custom empty destructor to forbid explicit copying of arenas;
otherwise copying storage would duplicate its namespace. Moving an arena
preserves its keys. `Key.duplicate` reconstructs three integers; it avoids the
fallible generic aggregate `copy` API and never duplicates a node payload.
The reduced generation limit is an experimental test seam for exhaustion, not
a proposed production tuning knob. Slots retire on exhaustion without wrapping.

## Two-node updates without simultaneous indexed loans

`update_two` validates first, moves both values into local owners, lends those
locals to the callback, then restores them before returning the callback result.
The exclusive arena borrow prevents the callback from re-entering that arena.
References cannot escape through the stored `Option[R]` result.

This does not allocate, copy payloads, or run their destructors during transfer.
It works for affine payloads. It does physically move values, potentially
several times for a large inline `T`; it does not promise stable addresses or
zero memory traffic. A callback returning `Err` still restores both values, with
its mutations intact. Fatal traps do not unwind; restoration after a process
abort is not promised.

Borrowing distinct indices directly remains rejected. The prototype shows a
library alternative, not a change to collection loans. It deliberately avoids
temporarily allocating a second container or requiring copied payloads.

## Failure and cost boundaries

Reusing a free slot and removing a value allocate nothing. Fresh slots use
fallible list append; existing contents and keys survive failure, while the
incoming value is destroyed. The free list lives in vacant slots, so removal
does not need an allocation. Retired slots are retained as empty storage.

Lookup, removal, and reuse take constant time apart from payload operations;
growth has list allocation/movement costs. Callback work and returned values
retain their own costs and failures. An arena does not enforce tree/graph
invariants, collect unreachable nodes, or repair incoming links.

## What is proved, and what is missing

Native tests cover successful access, deletion/reuse, stale and foreign-namespace
rejection, generation exhaustion, affine destruction, callback errors,
allocation-free reuse/pair access, and preservation during failed growth.
The demo and module are exercised by `tests/test_data_structure_patterns.rs`.

Namespaces are supplied integers. Two arenas given the same namespace can accept
one another's keys if the slot and generation coincide. Negative namespace
values are not special. This is an explicit prototype limitation, not safe
default instance identity. The production design must solve it before this can
become `std.arena`. The module also does not supply serialization, iteration,
compaction, shared concurrent access, or a node-address guarantee.

These findings support the generic arena direction while leaving its production
identity and access API open for review. Delivery and acceptance criteria live
in B73 of the [backlog](../../backlog.md).
