# Represent parent links with node IDs

When children need to identify their parent, one list can own all the nodes.
Links then contain indices, not references or owning boxes. This representation
is often called an *arena*. Moving the list's storage during growth does not
change an index.

This append-only example stores children as IDs too:

```plenty
class Node:
    name: str
    parent: Option[i64]
    children: list[i64]

def main() -> Result[(), AllocError | IoError]:
    mut nodes: list[Node] = []?
    nodes.append(Node("root", Nothing, []?))?
    nodes.append(Node("leaf", Some(0), []?))?
    nodes[0].children.append(1)?

    child = nodes[0].children[0]
    match nodes[child].parent:
        case Some(parent):
            print(nodes[parent].name)?
        case Nothing:
            print("no parent")?

    mut pending = [0]?
    while True:
        match pending.pop():
            case Some(id):
                print(nodes[id].name)?
                for child_id in &nodes[id].children:
                    pending.append(*child_id)?
            case Nothing:
                break
    Ok(())
```

```output
root
root
leaf
```

The explicit worklist avoids recursive function calls. It owns integers, so
it does not need to store references, which Plenty currently rejects. Its
growth is fallible and its memory use depends on the tree's shape. A graph
with cycles needs visited-node tracking as well.

Keep this list append-only while IDs are live. Removing an element from the
middle, sorting, or swapping elements changes which node an index denotes.
Removing a graph edge does not destroy its target: the arena owns that node
until explicitly removed or the whole arena is dropped. If IDs come from
outside this controlled example, check their range before indexing.

For reusable slots, tombstones preserve indices and generation checks can
reject stale IDs after reuse. Those invariants belong in a reusable abstraction;
raw integers do not enforce them. The [arena proposal](../../proposals/arenas.md)
contains a tested prototype, not an available `std.arena` module.

This example exits and drops the arena if construction fails. An API that
mutates a caller's existing graph must reserve needed storage before changing
parent/child links, or explicitly roll back partial changes. See
[failure-preserving insertion](recursive-insertion.md).
