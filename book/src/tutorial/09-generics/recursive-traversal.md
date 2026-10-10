# Optional: traverse recursive data

Traverse a chain without consuming it by matching a borrow. `Some(next)` below
binds `next` as `&Box[links.Node[i64]]`, which can be passed where a reference to
the node is expected.

```plenty-file links.plenty
pub class Node[T]:
    pub value: T
    pub next: Option[Box[Node[T]]]
```

```plenty
import links

def total(node: &links.Node[i64]) -> i64:
    match &node.next:
        case Some(next):
            node.value + total(next)
        case Nothing:
            node.value

def increment(node: &mut links.Node[i64]) -> ():
    node.value = node.value + 1
    match &mut node.next:
        case Some(next):
            increment(next)
        case Nothing:
            pass

def main() -> Result[(), Failure]:
    mut chain = links.Node[i64](3, Some(Box(links.Node[i64](6, Nothing))?))
    print(total(&chain))?
    increment(&mut chain)
    print(total(&chain))?
    print(chain.value)?
    Ok(())
```

```output
9
11
4
```

`match &mut value` binds mutable payload references, allowing `increment` to
update each node while its caller keeps the chain.

`increment` ends with a tail call through a reference borrowed from its parameter,
so its stack does not grow. `total` adds after its recursive call returns, so it
uses native stack space for every node.

A loop avoids that. Declare a reference binding with `mut` and assign it the
next node. Such a binding may only be assigned a reference borrowed from itself,
which `next` is: it comes from `match &node.next`.

```plenty
class Node:
    value: i64
    next: Option[Box[Node]]

def total(head: &Node) -> i64:
    mut sum = 0
    mut node = head
    while True:
        sum = sum + node.value
        match &node.next:
            case Some(next):
                node = next
            case Nothing:
                break
    sum

def increment(head: &mut Node) -> ():
    mut node = head
    while True:
        node.value = node.value + 1
        match &mut node.next:
            case Some(next):
                node = next
            case Nothing:
                break

def main() -> Result[(), Failure]:
    mut chain = Node(3, Some(Box(Node(6, Nothing))?))
    increment(&mut chain)
    print(total(&chain))?
    Ok(())
```

```output
11
```

The chain stays borrowed while `node` is used. The loop's stack usage is
independent of chain length.
