# Walk recursive data with a reference

A reference binding declared with `mut` can advance through a recursive
structure by borrowing each next step from itself:

```plenty
class Folder:
    name: str
    inside: list[Folder]

def innermost(top: &Folder) -> str:
    mut folder = top
    while len(folder.inside) > 0:
        folder = &folder.inside[0]
    folder.name

def main() -> Result[(), AllocError | IoError]:
    notes = Folder("notes", []?)
    work = Folder("work", [notes]?)
    home = Folder("home", [work]?)
    print(innermost(&home))?
    Ok(())
```

```output
notes
```

`folder` starts at `top`. Each `&folder.inside[0]` is borrowed from `folder`, so
the assignment is allowed. `home` stays borrowed until `folder`'s last use.

The binding can also be pointed at another place within the value it first
borrowed, without going through itself. This *retargets* it: the old loan ends
at the assignment. A search can remember its best element this way, without
keeping an index:

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

```output
banana
```

`best` first borrows from `words`, so `&words[i]` is a valid new target. See
[reference binding reassignment](../../design/language/05-ownership/03-reference-rebinding.md)
for the full rule. Pointing it at a different value is rejected, even one of the
same type:

```plenty-error
def main() -> Result[(), AllocError | IoError]:
    first = [1, 2]?
    second = [3]?
    mut view = &first
    view = &second
    print(view)?
    Ok(())
```

```error
reference binding `view` can be reassigned only to a reference into the owner it first borrowed
```

[Matching borrowed values](../06-data/08-borrowed-matching.md) also leaves the
owner in place while inspecting its contents.

The same descendant-cursor pattern can lend exclusive access while updating
each node. See the complete [mutable chain walk](../../design/examples/data-structures/linked-stack.md).
For branching traversal, a stack of references is currently unsupported; an
[arena with a worklist of IDs](arena-trees.md) is an alternative.
