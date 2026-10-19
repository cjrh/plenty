# Choose a representation for linked data

Start with the ownership and operations the application needs:

| Need | Starting representation | Important cost or constraint |
| --- | --- | --- |
| Stack or sequential data | `list[T]` | Contiguous storage; growth can allocate. |
| Queue with two ends | [`std.deque`](../../stdlib/deque.md) | Ring buffer; amortized constant-time insertion and constant-time removal. |
| Tree with uniquely owned children | Classes/enums with boxes or child lists | Subtrees move as owners; boxes allocate per node. |
| Parent/back links, shared targets, or cycles | List owning nodes plus IDs | IDs need validation; removal/reuse needs an identity policy. |
| Frequent rewiring at known nodes | Linked owners or arena links | Finding the node may dominate; measure allocation and locality costs. |

These are starting points, not performance rankings. The current box runtime
has a 32-byte header before the payload, in addition to allocator overhead and
alignment. Treat this as an implementation detail, not a stable ABI or proof
that one representation wins. See [memory architecture](../../toolchain/02-architecture/memory.md).

- [Push, pop, and walk an owned chain](linked-stack.md).
- [Rotate a tree without allocating new boxes](tree-rotation.md).
- [Build parent links and an iterative worklist](../../../tutorial/09-generics/arena-trees.md).

Boxes allow direct field access and transfer without explicit dereferences in
many contexts. They do not permit two owners of the same node. Current borrowing
rules also reject stored references and simultaneous mutable loans into distinct
indices of one list. Sequential updates such as `nodes[a].next = Some(b)` work;
the IDs themselves borrow nothing.

Automatic destruction of recursive owners uses bounded native stack space and
does not allocate. Recursive user traversal and destructor hooks do not inherit
that guarantee. A descendant reference cursor handles a chain; a non-consuming
general tree traversal needs a supported worklist representation. Arena IDs are
one option when a list of borrowed nodes is unavailable.

An *intrusive* collection embeds its links in the application object. It can
support multiple memberships and, in pointer-based designs, may need stable
addresses and externally managed lifetimes. Arena-owned objects with integer
links cover some of these uses, but do not promise stable physical addresses.
The [design assessment](../../../proposals/arenas.md) discusses this boundary
and the candidate standard-library abstraction.
