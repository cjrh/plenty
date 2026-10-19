# Rotate a tree without allocating new boxes

A right rotation moves the left child into the root position and transfers
its right subtree to the old root's left. Keep both nodes boxed: converting a
box to a `Tree` value would free that box and require allocation to box it again.

```plenty-file tree.plenty
pub class Tree:
    pub value: i64
    pub left: Option[Box[Tree]]
    pub right: Option[Box[Tree]]

pub def rotate_right(root: Box[Tree]) -> Box[Tree]:
    mut old = root
    match replace(old.left, Nothing):
        case Some(left):
            mut pivot = left
            old.left = replace(pivot.right, Nothing)
            pivot.right = Some(old)
            pivot
        case Nothing:
            old
```

```plenty
from tree import Tree, rotate_right

def main() -> Result[(), AllocError | IoError]:
    left = Box(Tree(1, Nothing, Nothing))?
    root = Box(Tree(2, Some(left), Nothing))?
    rotated = rotate_right(root)
    print(rotated.value)?
    match &rotated.right:
        case Some(right):
            print(right.value)?
        case Nothing:
            print("no right child")?
    Ok(())
```

```output
1
2
```

The rotation takes constant time and allocates nothing. If there is no left
child, it returns the original owner unchanged. It preserves in-order node
order; it does not by itself balance a tree or validate search-tree ordering.

Removing a subtree similarly uses `replace(root.left, Nothing)`. The returned
owner can be reattached or dropped. Parent references cannot be stored in these
nodes; use [arena IDs](../../../tutorial/09-generics/arena-trees.md) when parent
navigation is required.
