# Push, pop, and walk an owned chain

Each node owns its successor. Insertion allocates before transferring the old
head so a failed allocation preserves the caller's chain. Pop transfers the
successor out before destroying the removed node.

```plenty-file linked.plenty
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

```plenty
from linked import Linked, push, pop, bump

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

`cursor` advances by borrowing from its current referent. It does not create
stored references or require a recursive call. Pop and this traversal allocate
nothing. Each successful push allocates one box; its failure destroys the
incoming value, not the existing chain.

This is a teaching module with public links, not a packaged collection that
enforces an abstract API. Use a contiguous list for an ordinary stack unless
the workload benefits from separate nodes. See [representation choices](index.md).
