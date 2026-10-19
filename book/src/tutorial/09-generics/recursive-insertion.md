# Preserve a chain when insertion fails

A borrowed collection should usually keep its existing contents when insertion
fails. Allocate the new node before detaching the old head:

```plenty
class Node:
    value: i64
    next: Option[Box[Node]]

class Chain:
    head: Option[Box[Node]]

def push(chain: &mut Chain, value: i64) -> Result[(), AllocError]:
    mut node = Box(Node(value, Nothing))?
    node.next = replace(chain.head, Nothing)
    chain.head = Some(node)
    Ok(())

def main() -> Result[(), AllocError | IoError]:
    mut chain = Chain(Nothing)
    push(&mut chain, 10)?
    push(&mut chain, 20)?
    match &chain.head:
        case Some(head):
            print(head.value)?
        case Nothing:
            print("empty")?
    Ok(())
```

```output
20
```

The only allocation is `Box(...)`. After it succeeds, `replace` transfers the
old head and leaves `Nothing` behind; linking the nodes cannot fail or allocate.
If allocation fails, the existing chain has not changed.

Putting `replace(chain.head, Nothing)` inside the `Box` argument would detach
the chain first. Failed box allocation destroys its input, including that old
chain. Memory safety does not imply that a failed operation preserves data.
For a generic payload, also decide whether failure should destroy the incoming
value or return it to the caller.

The earlier [consuming construction example](recursive-data.md) abandons the
whole local computation on failure. This example has a stronger contract because
the caller retains the collection. See the complete
[linked-stack operations](../../design/examples/data-structures/linked-stack.md).
