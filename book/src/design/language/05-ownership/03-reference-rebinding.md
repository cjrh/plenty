# Reference binding reassignment

A mutable reference binding may be assigned only a reference borrowed from
itself: a field, element, box content, borrowed-match payload, or the result of
a reference-returning call that received it. Its type cannot change. Even a
fresh borrow of its original owner is rejected when it does not go through
the binding. The binding stays within its first borrowed target, and that
target remains borrowed until its last use.

```plenty
class Node:
    value: i64
    next: Option[Box[Node]]

def total(head: &Node) -> i64:
    mut sum = 0
    mut cur = head
    while True:
        sum = sum + cur.value
        match &cur.next:
            case Some(next):
                cur = next
            case Nothing:
                break
    sum

def main() -> Result[(), AllocError | IoError]:
    chain = Node(1, Some(Box(Node(2, Nothing))?))
    print(total(&chain))?
    Ok(())
```

This traversal uses constant stack space and allocates nothing after constructing
the chain. Because a reassignable reference can point anywhere within its first
target, loans through it protect that whole target. Two such field loans are
not considered disjoint. An exclusive binding cannot be accessed while a live
child reference conflicts with it. Immutable reference bindings keep precise
field loans.
