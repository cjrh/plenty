# Optional: traverse recursive data

Use `match &value` to inspect an enum while keeping its owner. Its payload
bindings are references: `Some(next)` below binds `next` as
`&Box[links.Node[i64]]`. A reference to a box can be passed where a reference
to its content is expected, so `total(next)` borrows the next node. This lets
us traverse a chain without taking it apart or allocating copies.

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

`match &mut value` binds mutable payload references. The recursive calls above
borrow the next node, and ordinary field assignment updates it in place. The
owner remains usable afterward and is dropped automatically at scope exit.
These traversals allocate nothing. `increment` ends with its recursive call and
passes on a reference borrowed from its own parameter, so that call is a tail
call and the stack does not grow. `total` adds to the result after its call
returns, so each of its calls still uses native stack space.

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

`node` starts at `head` and only moves deeper into the chain `head` borrows, so
the chain stays borrowed for as long as `node` is used. The loop uses the same
stack space for a chain of any length.
