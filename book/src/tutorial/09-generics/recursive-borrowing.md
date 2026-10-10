# Walk recursive data with a reference

Borrow named bindings and their class fields. Reference bindings must
be initialized with `&name`, `&mut name` (including field paths), another
reference, or a call returning a reference.

A reference binding declared with `mut` can be assigned again, under one rule:
the new reference must be borrowed from the binding itself. It can move further
into the value it already borrows, and nowhere else:

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

[Match borrowed values](../06-data/08-borrowed-matching.md) uses this rule to walk a
chain in a loop. References cannot
be stored in collections, captured by generators, or
remain live across `yield`. Element references such as `&items[0]` borrow named
collection storage. Use `next(&mut it)` through an exclusive generator reference when borrowing
a generator; generator iteration still consumes its owner.
