# Transfer or copy ownership

A list has one owner responsible for its mutable contents. Assigning it to
another binding transfers that responsibility:

```plenty
def main() -> Result[(), Failure]:
    original = [10, 20]?
    mut changed = original
    changed.append(30)?
    print(changed)?
    Ok(())
```

```output
[10, 20, 30]
```

The assignment moves the list. The old binding cannot be used afterward:

```plenty-error
def main() -> ():
    original = [10, 20].unwrap()
    changed = original
    print(original).unwrap()
```

```error
use of moved binding `original`
```

Passing a collection to an owned parameter or returning it also transfers
ownership. A move transfers the existing storage rather than duplicating it.

Use `copy(value)?` when you need independent contents. Copying observes the
source and returns `Result[T, AllocError]`, so the source remains usable
whether copying succeeds or fails. Nested mutable contents are copied too;
immutable strings may share their existing storage.

Numbers and booleans copy on assignment. Strings remain usable after
assignment because their storage is immutable. Tuples containing only these
values copy; tuples containing collections move.

A mutable owner can receive a new value after moving its previous one.
The compiler checks all continuing paths: a move on one branch can cause a
`use of possibly moved binding` diagnostic later.
