# Remove a child and clean up a recursive structure

To take an owned child out of a mutable class, leave a replacement behind.
`replace(node.next, Nothing)` returns the old tail without cloning or allocating:

```plenty
class Link:
    value: i64
    next: Option[Box[Link]]

def main() -> Result[(), Failure]:
    mut chain = Some(Box(Link(1, Some(Box(Link(2, Nothing))?)))?)
    while True:
        match chain:
            case Some(link):
                mut node = link
                print(node.value)?
                chain = replace(node.next, Nothing)
            case Nothing:
                break
    Ok(())
```

```output
1
2
```

The old tail becomes `chain`; dropping `node` now sees an empty `next` field.
Installing another tail instead of `Nothing` also lets you relink existing nodes.
`replace` requires a mutable field and returns its previous value directly.
It evaluates the replacement before any destination indices, just like assignment.

Leaving scope drops the whole owned structure automatically. Automatic cleanup
uses an allocation-free queue for boxes, so its native stack usage does not grow
with the number of nodes in a chain. If you write a recursive traversal function
yourself, ordinary function-call stack limits still apply; the loop above avoids
that. A program that runs out of stack prints `error: stack overflow` and stops.

To inspect a structure while keeping it, [traverse it through a borrow](recursive-traversal.md).
Moving individual owned fields out without replacing them remains unsupported.
Automatic `copy`, equality, `print`, and `str.repr` also reject recursive data,
including containers holding it. Print selected scalar fields as above, or write
an explicit traversal. These restrictions prevent the existing recursive runtime
operations from overflowing the stack on otherwise valid deep values.
