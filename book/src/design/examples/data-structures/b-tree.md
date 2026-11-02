# Split, borrow, and merge in a B-tree

The [complete B-tree implementation](balanced-trees/btree.plenty) is a
minimum-degree-2 B-tree, or **2–3–4 tree**. Its nonempty root and every other
live node contain one to three keys. An internal node with `n` keys has `n + 1`
children; all leaves have the same depth. Unlike a B+ tree, keys can live in
internal nodes, and leaves have no sibling chain.

Use the [balanced-tree demo and checks](balanced-trees.md) to run it.

## Storage

Each node owns two fixed-length lists: three key slots and four child-ID slots.
`n` selects the live key prefix. Unused key slots may retain old scalar values;
unused child slots in live nodes are `-1`. A leaf has no child IDs. The root,
size, free-list head, and node storage belong to one private `Tree` owner.

The free pool retains deleted nodes and their two lists. Taking a slot resets
its child IDs; releasing a slot adds it to the pool without moving other nodes.
No public operation exposes a node ID.

## Insert: split before descending

`insert` first searches for a duplicate. For a new key, it measures the current
height and prepares enough spare nodes for every possible split on the descent.
A full root needs two slots: a new root and a right sibling. Each remaining
level needs at most one sibling. With height counted in node levels, reserving
`height + 1` spare nodes covers the empty tree and both cases.

Preparation is the only fallible mutation phase. It allocates detached empty
nodes and links successful allocations into the free pool. Failure can retain
extra spare storage, but does not change the root, reachable shape, keys, or
size. A retry can reuse those spares. Once preparation succeeds, rewiring,
key shifts, and taking spare nodes allocate nothing.

For a full child `[a, b, c]`, `split`:

1. Leaves `[a]` in the original node.
2. Moves `[c]` and the appropriate children into a spare sibling.
3. Moves `b` into the parent, shifting parent keys and child IDs to make room.

Splitting a full root first ensures the parent always has room. Splitting each
full child before descending means insertion reaches a nonfull leaf, where an
in-place key shift creates the final slot.

## Delete: do not descend into a one-key node

`remove` first confirms membership, so a missing-key removal changes nothing.
The internal `erase` helper therefore assumes it will find its key.

At a leaf, shift keys left and decrement `n`. At an internal matching key:

- If the left child has at least two keys, replace the key with its predecessor
  and delete that predecessor from the left subtree.
- Otherwise, if the right child has at least two keys, use its successor.
- Otherwise, merge the two one-key children with the separating key, then
  delete within the merged three-key node.

When descending toward a key, a one-key child needs help first:

- **Borrow from the left:** move the parent's separator into the child's first
  slot, move the sibling's largest key into the parent, and transfer the
  sibling's last child if these are internal nodes.
- **Borrow from the right:** perform the symmetric transfer using the sibling's
  smallest key and first child.
- **Merge:** if neither sibling can lend, join two one-key children around a
  parent separator. Return the unused sibling slot to the free pool.

This preserves the minimum occupancy without a separate upward underflow pass.
If the root becomes empty, its only child becomes the root; deleting the final
leaf key makes the tree empty. The old root slot is released in both cases.

## Costs and validation

At fixed degree, search and deletion take `O(log n)` time and deletion allocates
nothing. Insertion performs `O(log n)` tree work and prepares at most `O(log n)`
spare nodes; occasional backing-list growth costs `O(n)`, giving amortized
`O(log n)` insertion rather than a worst-case bound. `ordered` visits every key
in `O(n)` time and allocates its result. Recursive deletion/traversal uses
`O(log n)` native stack space for trees maintained by these operations.

Storage follows the historical peak plus the split reserve, not just the current
key count. `valid` allocates and scans visitation marks for all retained slots,
so its cost follows retained storage. It verifies occupancy, ordered bounds,
equal leaf depth, active child counts, total size, and free-pool membership.

The [test driver](balanced-trees/checks.plenty) exercises both borrow directions,
predecessor/successor replacement, merges, root changes, and reuse through
permutation, ordered, and mixed histories. These are tests of a teaching
implementation, not performance qualification for a database or filesystem.

## Acknowledgements

- Cormen, Leiserson, Rivest, and Stein,
  [*Introduction to Algorithms*](https://mitpress.mit.edu/9780262046305/introduction-to-algorithms/):
  the published B-tree split and top-down borrow/merge algorithms. The Plenty
  implementation is original; no external source code or data is bundled.
