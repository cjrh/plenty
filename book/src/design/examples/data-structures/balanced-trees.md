# Build balanced search trees

A search tree keeps keys ordered while its shape changes. These examples
implement two balanced sets directly in Plenty:

- A [B-tree](b-tree.md) with minimum degree 2: each node holds up to three
  keys and four children, also called a **2–3–4 tree**.
- A [left-leaning red-black tree](red-black-tree.md): each node holds one key,
  two child IDs, and a color bit.

Both implement insertion, membership, deletion, length, and sorted traversal.
They store distinct `i64` keys, including the full signed range. Duplicate
insertion and missing-key removal return `False` without changing the tree.
These are educational implementations, not `std` APIs or benchmark results.
Use a built-in `set[T]` when ordering is unnecessary.

## Run the implementations

From a repository checkout:

```sh
plenty book/src/design/examples/data-structures/balanced-trees/main.plenty
plenty book/src/design/examples/data-structures/balanced-trees/checks.plenty
```

Or save the [B-tree module](balanced-trees/btree.plenty),
[red-black module](balanced-trees/red_black.plenty), and
[demo](balanced-trees/main.plenty) in the same directory, then run
`plenty main.plenty`. Save [checks.plenty](balanced-trees/checks.plenty) there
to run the invariant and model checks as well.

The included source is the actual demo compiled by the integration tests:

```plenty fragment
{{#include balanced-trees/main.plenty}}
```

Expected output:

```text
[10, 20, 30, 40, 50, 60, 70]
[10, 20, 30, 40, 50, 60, 70]
False False
True False
True True
False False
[10, 20, 30, 50, 60, 70]
[10, 20, 30, 50, 60, 70]
True True
```

## Shared interface and ownership

Both modules expose the same operations:

| Operation | Result | Allocation |
| --- | --- | --- |
| `create()` | `Result[Tree, AllocError]` | Empty owner creation can fail. |
| `insert(&mut tree, key)` | `Result[bool, AllocError]` | May grow storage; `True` means newly inserted. |
| `has(&tree, key)` | `bool` | None. |
| `remove(&mut tree, key)` | `bool` | None; `True` means removed. |
| `length(&tree)` | `i64` | None. |
| `ordered(&tree)` | `Result[list[i64], AllocError]` | Allocates an owned sorted result. |
| `valid(&tree)` | `Result[bool, AllocError]` | Allocates visitation marks for diagnostics. |

A `Tree` exclusively owns a private list of nodes. Links are private indices,
not references or owning edges. `-1` means no child; it is never used as a
list index by a valid operation. Updating links in separate statements avoids
simultaneous mutable loans into the same list. Neither implementation needs
stored references, parent pointers, unsafe operations, or a foreign library.

Deleted nodes join an internal free list. Reusing them needs no generation
counter because node IDs never leave the module. Surviving IDs remain valid
when the backing list grows. Storage is retained for reuse rather than shrunk;
dropping the tree releases all of it. Neither type is a copyable value.

## Validate more than sorted output

`checks.plenty` compares both trees against a sorted-list model after every
operation and runs each implementation's structural validator. It checks:

- All **14,400 pairs** of insertion and deletion permutations for five keys,
  including `i64`'s minimum and maximum; repeated insertions, repeated removals,
  empty trees, and refill after complete deletion.
- Four 257-key scenarios using ascending, descending, and alternating-extreme
  orders, including root growth and contraction, followed by slot reuse.
- **16,000 deterministic mixed operations** across four seeds, followed by
  draining every remaining key.

The validators check strict ordering, key counts, reachability, duplicate or
cyclic links, and separation of reachable nodes from the free list. The B-tree
also checks occupancy and equal leaf depth; the red-black tree checks its color
and black-height invariants.

The Rust integration harness additionally sweeps allocation-failure points
before insertion, checks retry after failure, and verifies deletion and reuse
with allocation disabled:

```sh
cargo test --locked --features runtime-checks --test integration test_data_structure_patterns::balanced_tree
```

These tests are regression evidence, not a proof over every possible history.
The validators assume storage created by the module, not hostile serialized
input. Recursive traversal and validation use native stack proportional to tree
height; ordinary balanced operations keep that height logarithmic.

## Deliberate limits

There are no generic comparators, key/value maps, range cursors, rank/select,
persistence, concurrency, disk pages, or externally reusable handles. The
B-tree's tiny fixed degree makes split/merge cases readable; its separately
allocated per-node key and child lists are not a cache-tuned storage design.
Growing either backing list can move many rows, so insertion is not a strict
worst-case logarithmic-time operation in this representation.

See [representation choices](index.md) and the
[linked-list lesson](../../../tutorial/04-collections/linked-lists.md) for the
ownership trade-offs behind index-based links.
