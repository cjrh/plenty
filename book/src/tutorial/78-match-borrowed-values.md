# Match borrowed values

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
These traversals allocate nothing; their recursive calls still use native stack
space.

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

An existing enum reference can be matched directly. A function can also return
a payload reference when it comes from the function's single reference parameter:

```plenty
def number(value: &Result[i64, i64]) -> &i64:
    match value:
        case Ok(payload):
            payload
        case Err(payload):
            payload

def main() -> Result[(), Failure]:
    mut outcome: Result[i64, i64] = Err(7)
    selected = number(&outcome)
    print(*selected)?
    outcome = Ok(9)
    print(number(&outcome))?
    Ok(())
```

```output
7
9
```

`*selected` reads the scalar through its reference. `print` can also observe a
reference directly. Replacing `outcome` is allowed after `selected`'s last use;
doing it before that use is rejected. References cannot outlive a local owner.

For mutable scalar payloads, assignment through the reference changes the original
slot. Nested standard sums preserve their enclosing variants:

```plenty
def main() -> Result[(), Failure]:
    mut nested: Option[Result[i64, str]] = Some(Ok(4))
    match &mut nested:
        case Some(result):
            match result:
                case Ok(number):
                    *number = *number + 1
                case Err(_):
                    pass
        case Nothing:
            pass
    print(nested)?
    Ok(())
```

```output
Option[Result[i64, str]].Some(Result[i64, str].Ok(5))
```

Mutable matching works for every enum, including recursive ones. `_` ignores a
payload without moving it.
Borrowed matching uses the same exhaustiveness checks and qualified user-variant
names as owned matching.
