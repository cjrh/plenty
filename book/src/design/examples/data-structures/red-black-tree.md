# Rotate and recolor a red-black tree

The [complete red-black implementation](balanced-trees/red_black.plenty) uses
**left-leaning red-black (LLRB)** links. It is a red-black tree with an extra
orientation rule, not the parent-pointer variant with symmetric insertion and
double-black deletion cases. Use the [demo and checks](balanced-trees.md) to
run it alongside the B-tree.

## Invariants

Each node has one `i64` key, two child IDs, and a `red` bit describing its
incoming link. Missing children are black. After each public operation:

- The root is black.
- Keys obey strict binary-search ordering.
- No red link leans right.
- A red node has no red child.
- Every path from a node to a missing child has the same number of black links.

A black node connected to a left red child represents a logical three-node
containing two keys. Collapsing these red links gives a balanced **2–3 tree**.
The [degree-2 B-tree](b-tree.md) instead allows three keys in a node; the two
examples are related balanced representations, not identical encodings.

## Ownership and rotations

The tree owns a list of private scalar-field nodes. Child links are indices,
so a rotation updates existing nodes in place without allocating boxes or
storing references. Deleted slots join a private free list and are reset before
reuse.

A left rotation promotes the right child, transfers its left subtree to the
old root's right, and makes the old root its left child. The promoted node
inherits the old root's color; the old root becomes red. A right rotation is
symmetric. These changes preserve key order but are only part of balancing.

`flip` toggles a node's color and both child colors. Unlike setting colors to
constants, toggling supports both splitting and joining logical nodes. Internal
callers ensure both children exist before invoking it.

## Insert

After rejecting a duplicate, `insert` allocates or reuses one detached red node
**before changing any tree links or colors**. Failed growth therefore preserves
the existing set, shape, and size.

The recursive `put` links that node at its binary-search position, then calls
`balance` while returning toward the root. The repair sequence is:

1. Rotate left for a red right link.
2. Rotate right for two consecutive left red links.
3. Flip colors when both children are red.

The shared repair helper also handles deletion's transient shapes. The public
operation finally paints the root black. Once the detached node exists, this
phase allocates nothing.

## Delete

`remove` first confirms membership, so its internal descent can assume the key
exists. If both root children are black, it temporarily paints the root red to
permit a color transfer without reducing the final black height.

Before descending into a black child whose left child is also black,
`move_red_left` or `move_red_right` transfers a red link toward that child.
Color flips merge logical nodes temporarily; rotations can borrow from a
sibling. These are top-down preparations, not an upward double-black repair.

When deleting a matching internal key, replace it with the smallest key in its
right subtree, then use `delete_min` to remove that successor node. A matching
terminal node is released to the free list. `balance` repairs links on return,
and the remaining root is painted black. Deletion and all its rotations and
color changes allocate nothing.

## Costs and validation

Balancing bounds height by `O(log n)`. Membership and removal take `O(log n)`
time; recursive update and traversal use `O(log n)` native stack space.
Insertion performs logarithmic tree work, but growing the backing list can
move `O(n)` rows, so the representation provides amortized `O(log n)` insertion,
not a strict worst-case bound. Sorted collection visits `O(n)` keys and
allocates an owned list.

Free slots retain storage up to the historical peak. `valid` allocates and
scans visitation marks across that retained storage. It independently checks
ordering, black root, no right red link, no consecutive red links, equal black
height, size, reachability, and free-list separation.

See the [model and invariant tests](balanced-trees.md#validate-more-than-sorted-output)
for permutations, mixed operations, allocation failures, and allocation-free
removal and reuse. This implementation is intentionally a set of integer keys,
without generic comparators, ranks, or external node handles.

## Acknowledgements

- Robert Sedgewick and Kevin Wayne,
  [*Algorithms*: Balanced Search Trees](https://algs4.cs.princeton.edu/33balanced/):
  the published left-leaning representation, rotations, color flips, and
  top-down deletion algorithm. The Plenty implementation is original; no
  external source code or data is bundled.
