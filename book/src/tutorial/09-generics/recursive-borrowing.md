# Walk recursive data with a reference

A reference binding declared with `mut` can advance through a recursive
structure. Each new reference must be borrowed from the binding itself:

```plenty
class Folder:
    name: str
    inside: list[Folder]

def innermost(top: &Folder) -> str:
    mut folder = top
    while len(folder.inside) > 0:
        folder = &folder.inside[0]
    folder.name

def main() -> Result[(), Failure]:
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

Pointing it at a different value is rejected, even one of the same type:

```plenty-error
def main() -> ():
    first = [1, 2].unwrap()
    second = [3].unwrap()
    mut view = &first
    view = &second
    print(view).unwrap()
```

```error
reference binding `view` can be reassigned only to a reference borrowed from `view` itself
```

[Matching borrowed values](../06-data/08-borrowed-matching.md) also leaves the
owner in place while inspecting its contents.
