# Store recursive children in a list

Classes can own recursive children too. A list of children already keeps them
on the heap, so it needs no box. This companion module declares a generic tree
node with public fields:

```plenty-file forest.plenty
pub class Node[T]:
    pub value: T
    pub children: list[Node[T]]
```

```plenty
import forest

def main() -> Result[(), Failure]:
    mut root = forest.Node[i64](1, [forest.Node[i64](2, []?)]?)
    root.children[0].value = 9
    print(root.value)?
    print(root.children[0].value)?
    Ok(())
```

```output
1
9
```

The nested assignment updates the existing child without copying or allocating
another node. An empty children list terminates this tree. For a single optional
child, a field such as `next: Option[Box[Node]]` can terminate with `Nothing`.
Classes and enums may also refer to one another through aliases.
