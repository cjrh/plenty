# Reference binding reassignment

A mutable reference binding covers the place it first borrowed, its *first
place*. A list element's first place is the whole list. The binding may be
assigned, with an unchanged type:

- a reference borrowed through itself: a field, element, box content,
  borrowed-match payload, or the result of a reference-returning call that
  received it. The binding's loan continues.
- another reference into its first place that does not go through the binding,
  such as `cur = head` or `best = &items[i]`. This *retargets* the binding: the
  loan it held ends at the assignment, and later uses hold the new loan.

A reference to a different owner, or to a place of the same owner outside the
first place (`&p.right` after `&mut p.left`), is rejected. The first place is
borrowed while the binding is live, so the owner cannot change between a
retarget and a later use of the binding. On a path that skips a retarget, the
old loan continues; a reborrow of the old target keeps its own loan.

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

Retargeting lets a search remember its best element without an index:

```plenty
def longest(words: &list[str]) -> &str:
    mut best = &words[0]
    for i in range(1, len(words)):
        if words[i].byte_len() > best.byte_len():
            best = &words[i]
    best

def main() -> Result[(), AllocError | IoError]:
    words = ["fig", "banana", "kiwi"]?
    print(longest(&words))?
    Ok(())
```

Because the old loan ends at a retarget, a collection can grow after the old
target's last use and before the binding is pointed at a new element:

```plenty
def main() -> Result[(), AllocError | IoError]:
    mut values = [1, 2]?
    mut slot = &mut values[0]
    *slot = 10
    values.append(3)?
    slot = &mut values[2]
    *slot = 30
    print(values)?
    Ok(())
```
