# Build singly and doubly linked lists

A linked list connects nodes rather than treating adjacent storage positions as
adjacent elements. It is a useful way to see ownership and borrowing at work.
For an ordinary stack, prefer `list[T]`; for a queue with two ends, prefer
[`std.deque`](deque.md). Separate heap allocations and pointer chasing often
make linked nodes more expensive than contiguous storage.

These examples preview [classes](../06-data/01-records-and-methods.md): a
`class` groups typed fields, and its generated constructor takes those fields
in declaration order. `node.value` accesses a field. The `pub` declarations
make the teaching modules and their fields accessible to the example programs;
they are not encapsulated collection APIs.

## A singly linked list owns its successors

Each node owns one value and optionally a boxed successor. `Box(value)` moves
its argument into a heap allocation and returns a `Result`; `Nothing` ends the
chain. The box gives this recursive type a finite size.

```plenty-file singly.plenty
pub class Node:
    pub value: i64
    pub next: Option[Box[Node]]

pub class Linked:
    pub head: Option[Box[Node]]

pub def push(xs: &mut Linked, value: i64) -> Result[(), AllocError]:
    mut node = Box(Node(value, Nothing))?
    node.next = replace(xs.head, Nothing)
    xs.head = Some(node)
    Ok(())

pub def pop(xs: &mut Linked) -> Option[i64]:
    match replace(xs.head, Nothing):
        case Some(node):
            mut removed: Node = node
            xs.head = replace(removed.next, Nothing)
            Some(removed.value)
        case Nothing:
            Nothing

pub def bump(xs: &mut Linked) -> ():
    match &mut xs.head:
        case Some(head):
            mut cursor: &mut Node = head
            while True:
                cursor.value += 1
                match &mut cursor.next:
                    case Some(next):
                        cursor = next
                    case Nothing:
                        break
        case Nothing:
            pass
```

`replace(place, replacement)` installs a new value and returns the old one.
It lets `push` take the head without leaving the borrowed list uninitialized.
Allocate the detached node **before** taking that head: if allocation fails,
the caller still owns the original chain. Each successful push allocates one
box; popping and walking allocate nothing.

In `pop`, the annotation `removed: Node` extracts the contents of the owned box
and frees its allocation. Taking `removed.next` transfers the remaining chain
back to the list before the removed node is destroyed.

In `bump`, matching `&mut` borrows rather than consumes each link. A borrowed
box converts to a reference to its contents, so `cursor` can advance from one
node to its successor without copying or recursion. This is a preview of
[borrowed matching](../06-data/08-borrowed-matching.md).

```plenty
from singly import Linked, push, pop, bump

def main() -> Result[(), AllocError | IoError]:
    mut xs = Linked(Nothing)
    for value in range(1, 4):
        push(&mut xs, value)?
    bump(&mut xs)
    print(pop(&mut xs))?
    print(pop(&mut xs))?
    print(pop(&mut xs))?
    print(pop(&mut xs))?
    Ok(())
```

```output
Option[i64].Some(4)
Option[i64].Some(3)
Option[i64].Some(2)
Option[i64].Nothing
```

Push and pop operate at the head, so this example behaves as a stack. Each
operation takes constant time; walking takes time proportional to the number
of nodes. Dropping a remaining chain uses bounded native stack space and
allocates nothing. Automatic printing, equality, and copying of recursive
owners are unsupported; inspect values by traversing the chain instead.

## A doubly linked list uses IDs for both directions

Adding an owning `prev: Box[Node]` would make neighbours own each other. Plenty
does not support ownership cycles or references stored in fields. Instead,
let a built-in list own every node, and use integer indices as node IDs:

```plenty-file doubly.plenty
pub class Node:
    pub value: i64
    pub prev: Option[i64]
    pub next: Option[i64]

pub class Linked:
    pub nodes: list[Node]
    pub head: Option[i64]
    pub tail: Option[i64]

pub def push_back(xs: &mut Linked, value: i64) -> Result[(), AllocError]:
    id = len(xs.nodes)
    xs.nodes.append(Node(value, xs.tail, Nothing))?
    match xs.tail:
        case Some(old_tail):
            xs.nodes[old_tail].next = Some(id)
        case Nothing:
            xs.head = Some(id)
    xs.tail = Some(id)
    Ok(())
```

The empty list has no head or tail. Appending creates a node whose `prev` is
the old tail, connects that tail's `next`, and then updates the tail ID. The
first node also becomes the head. Append happens before existing links change,
so allocation failure leaves them intact.

IDs borrow nothing. Updating neighbouring nodes in separate statements avoids
holding simultaneous mutable references into the backing list. Growing that
list may move nodes in memory, but their indices remain valid.

```plenty
from doubly import Linked, push_back

def main() -> Result[(), AllocError | IoError]:
    mut xs = Linked([]?, Nothing, Nothing)
    push_back(&mut xs, 10)?
    push_back(&mut xs, 20)?
    push_back(&mut xs, 30)?

    mut cursor = xs.head
    while True:
        match cursor:
            case Some(id):
                print(xs.nodes[id].value)?
                cursor = xs.nodes[id].next
            case Nothing:
                break

    cursor = xs.tail
    while True:
        match cursor:
            case Some(id):
                print(xs.nodes[id].value)?
                cursor = xs.nodes[id].prev
            case Nothing:
                break
    Ok(())
```

```output
10
20
30
30
20
10
```

Forward traversal follows `next`; backward traversal follows `prev`. This is a
doubly linked list logically, even though links are indices rather than machine
pointers. Appending is amortized constant time because the backing list can
need to grow; each traversal is linear.

### Keep the teaching limits explicit

This example supports append and traversal, not deletion or reusable public
handles. It relies on valid IDs and consistent links; public fields let callers
break those invariants. Invalid indices trap, and a cycle in the links could
make traversal loop forever.

Do not remove a backing-list entry with `pop(id)`: shifting later entries would
invalidate their IDs. To support deletion, use vacant slots, for example a
`list[Option[Node]]`, and reconnect the removed node's neighbours. If slots are
reused and callers retain IDs, add a generation check to reject stale handles;
handles crossing between collections also need an identity policy. This is more
than adding a `prev` field to an owned chain.

See [representation choices](../../design/examples/data-structures/index.md)
for trade-offs, [recursive storage](../../design/language/03-types/recursive-data.md)
for ownership limits, and [parent links with arena IDs](../09-generics/arena-trees.md)
for another use of index-based links.
