# 22. Borrow instead of transferring ownership

A shared reference, `&T`, permits observation. An exclusive reference, `&mut T`,
permits mutation. The owner remains responsible for cleanup. Function signatures
state which access is needed:

```plenty
def total(values: &list[i64]) -> i64:
    mut result = 0
    for value in values:
        result = result + value
    result

def add(values: &mut list[i64], value: i64) -> Result[(), AllocError]:
    values.append(value)?
    Ok(())

def main() -> Result[(), Failure]:
    mut numbers = [1, 2]?
    print(total(numbers))?
    add(&mut numbers, 3)?
    print(numbers)?
    Ok(())
```

```output
3
[1, 2, 3]
```

References can also be local bindings. `&mut` grants access to the target; it
does not make the binding itself reassignable. Use `*reference` to read a scalar or replace
the target. Collection operations such as `append`, indexing, and `len` work
through references directly.

The parameter determines borrowing: `total(numbers)` implicitly lends `numbers`
because `total` expects `&list[i64]`. Writing `total(&numbers)` also works.
Use explicit `&mut` for mutable arguments, as in `add(&mut numbers, 3)`.
An owned parameter still takes ownership. Existing reference arguments are
reborrowed without needing another marker.

```plenty
def main() -> Result[(), IoError]:
    mut score = 10
    reference = &mut score
    *reference = *reference + 5
    print(*reference)?
    score = 20
    print(score)?
    Ok(())
```

```output
15
20
```

The exclusive borrow ends after the last use of `reference`, so assigning to
`score` afterward is allowed. A reference that will be used later keeps its loan
live, including across branches and loop iterations:

```plenty-error
def main() -> ():
    mut numbers = [1, 2].unwrap()
    view = &numbers
    numbers.append(3).unwrap()
    print(view).unwrap()
```

```error
conflicting borrow: cannot modify or exclusively borrow `numbers` while it is borrowed
```

The diagnostic points at `numbers.append(3)`. Its notes point at `&numbers`,
where the shared borrow starts, and at `print(view)`, the later use that keeps
the borrow live.

A reference can be reborrowed temporarily. An exclusive reference may lend shared
or exclusive access, but its conflicting access is suspended while that child
borrow is live. Reference arguments automatically reborrow an existing reference;
they do not transfer the referenced owner.

When a conflict goes through a reference, the diagnostic names both the owner
and the reference written at the failing use:

```plenty-error
def main() -> ():
    mut numbers = [1, 2].unwrap()
    reference = &mut numbers
    view = &reference
    reference.append(3).unwrap()
    print(view).unwrap()
```

```error
conflicting borrow: cannot modify or exclusively borrow `numbers` (through `reference`) while it is borrowed
```

This subset borrows named bindings and their class fields. Reference bindings must
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

[Match borrowed values](78-match-borrowed-values.md) uses this rule to walk a
chain in a loop. References cannot
be stored in collections, captured by generators, or
remain live across `yield`. Element references such as `&items[0]` borrow named
collection storage. Use `next(&mut it)` through an exclusive generator reference when borrowing
a generator; generator iteration still consumes its owner.

Functions can return references when exactly one parameter is a reference. The
returned value must borrow that parameter, never a local owner. The caller keeps
the original value alive and borrowed until the result's last use.

```plenty
class Pair:
    left: i64
    right: i64
    def left_ref(self: &mut Pair) -> &mut i64:
        &mut self.left

def main() -> Result[(), Failure]:
    mut pair = Pair(1, 2)
    left = pair.left_ref()
    *left = 8
    print(pair.left)?
    Ok(())
```
```output
8
```

This simple getter preserves the returned field's identity, so unrelated fields
remain available after the call. More complex getters conservatively protect the
whole argument. A mutable returned reference requires a mutable reference parameter.
Use a named receiver, class field, or element of named storage; temporary receivers
cannot supply a reference that outlives their owner.

Shared arguments can also be temporary values, such as `size("ada")` or
`size(&"ada")` for a `size(s: &str)` function. The temporary is evaluated once
and kept alive through the entire expression. A result borrowing that temporary
may be used immediately, but cannot be saved in a binding or returned. Mutable
temporaries are not supported.
