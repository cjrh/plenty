# Trie storage, updates, and costs

The [runnable trie](trie.md) uses a private `list[Node]`. Each node contains a
one-scalar edge label, a parent ID, first-child and next-sibling IDs, an optional
stored key, and free-list bookkeeping. The root occupies slot zero and has no
edge label. `-1` means no link, never a valid index to dereference.

## Ordered child links

A parent's children form a singly linked list ordered by scalar label. To find
an edge, scan siblings until its label matches or exceeds the desired scalar.
This keeps storage and rewiring simple, but costs a linear scan in the parent's
fanout; it is not the constant-time edge lookup sometimes assumed for tries.

One owner controls all nodes. IDs borrow nothing, and each mutation updates one
field at a time. Backing-list growth can move rows without changing IDs. Parent
links support upward pruning and iterative traversal without stored references
or a stack of borrowed nodes. No node ID escapes the module, so slot reuse needs
no public generation or cross-collection identity scheme.

## Insert without damaging the trie on allocation failure

Insertion makes two streaming passes over the input scalars:

1. Follow existing edges until the first missing edge, then count the remaining
   missing suffix nodes. An existing terminal returns `False` immediately.
2. After storage preparation succeeds, walk again, insert the missing edges in
   sibling order, and set the final node's stored key.

Preparation appends empty nodes to a detached free pool until it has enough
slots for the entire missing suffix. Failure may retain spare capacity and free
nodes, but cannot change reachable links, terminals, or key count. A retry can
reuse successful partial preparation. Taking slots, rewiring, and copying an
immutable string handle allocate nothing once preparation succeeds.

Inserting the empty key, or marking an already-existing prefix node terminal,
needs no new nodes. No repeated `key[index]` scanning is used: string iteration
visits Unicode scalars directly.

## Remove and prune without allocating

Locate the terminal; missing paths and nonterminal prefixes return `False`.
Clear its stored key, then walk upward while the current node is a nonterminal
leaf. Unlink that node from its parent's child list, clear its fields, and put
its slot on the free list. Stop at the permanent root or a node still needed by
another key.

For example, removing `"car"` leaves the path for `"cart"`. Removing `"cart"`
afterward may prune the `r` and `t` suffix, but must preserve the shared `ca`
path when `"cat"` is stored. Removing `""` changes only the root's terminal.
No backing-list entry is removed or shifted.

## Prefix and longest-prefix traversal

Prefix enumeration first locates the prefix node. It then performs iterative
preorder over that subtree: emit the terminal before visiting ordered children,
use sibling links to advance, and parent links to backtrack. Stop before
following the starting node's sibling. This produces lexicographically ordered
keys without sorting or rebuilding text, and without crossing into unrelated
prefixes. The empty prefix starts at the root and visits the whole trie.

Longest-prefix matching follows the query's scalar edges while remembering the
last terminal encountered. It returns that terminal's stored string handle, not
a newly allocated slice of the query. The root terminal naturally handles an
empty-string fallback.

All production operations are iterative. Trie depth follows key length, so the
logarithmic-height stack bound of a balanced search tree would not be sufficient
here. The validator also uses an explicit heap worklist, not recursion.

## Complexity and retained memory

Let `m` be the input's scalar length and `d` the maximum fanout encountered.
Membership, prefix existence, longest-prefix lookup, and the tree work of
insertion/removal are bounded by `O(m × d)`, plus streaming the input. Pruning
also scans affected sibling lists. A missing-suffix insert may prepare `O(m)`
slots; occasional backing-list growth can move all retained rows, so it is not
strictly bounded by key length on each call.

After locating a prefix, enumeration takes `O(v + k)` work for `v` visited nodes
and `k` emitted handles, plus result-list allocation. Its traversal state uses
constant native stack and no auxiliary traversal list. Output is unbounded:
callers asking for the empty prefix collect the whole set.

Storage follows the historical peak number of scalar-prefix nodes, including
unused slots from partial preparation, rather than the current number of keys.
Deletion releases stored string handles and clears labels in pruned nodes but
retains node capacity for reuse. Terminal keys also retain their string storage;
this example is not a compressed radix trie or a minimal dictionary automaton.

`valid` allocates marks/workspace over retained storage. It checks sorted unique
sibling labels, valid live IDs, parent consistency, reachability without cycles
or duplicate edges, no unpruned nonterminal leaves, terminal paths and key count,
and free/live separation. It then searches each stored key to verify its path,
so its cost includes all those searches; it is not a constant-time health check
or a parser for hostile serialized state.

## Deliberate limits

This is a set, not a generic key/value map. There are no result limits, lazy
cursors, wildcard/fuzzy search, compressed edges, persistence, concurrency, or
normalization policy. A sorted child vector, dictionary, or radix representation
could trade allocation and implementation complexity for faster high-fanout
queries; measure a workload before choosing one. No new standard-library
contract or language feature is needed for this example.
