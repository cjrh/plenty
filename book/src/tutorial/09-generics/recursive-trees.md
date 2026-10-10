# Store recursive children in a list

A list of children provides the indirection a recursive class needs; each
child needs no additional box. Save this module as `forest.plenty`:

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

An empty children list terminates the tree. For a single optional child, use a
field such as `next: Option[Box[Node]]` and terminate it with `Nothing`.
