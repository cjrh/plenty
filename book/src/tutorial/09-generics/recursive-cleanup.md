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

Replacing `node.next` with `Nothing` leaves that field empty when `node` drops.
Use another tail as the replacement to relink nodes. `replace` requires a mutable
field and evaluates the replacement before any destination indices.

Automatic cleanup can drop a deep boxed chain without growing the native stack.
A recursive traversal you write can still overflow it; use a loop for unbounded
depth.

To inspect a structure while keeping it, [traverse it through a borrow](recursive-traversal.md).
Moving individual owned fields out without replacing them remains unsupported.
Automatic `copy`, equality, `print`, and `str.repr` also reject recursive data,
including containers holding it. Print selected scalar fields as above, or write
an explicit traversal.
