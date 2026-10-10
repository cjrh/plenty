# Borrow instead of transferring ownership

A shared reference, `&T`, permits observation. An exclusive reference, `&mut T`,
permits mutation. The owner remains responsible for cleanup. Function signatures
state which access is needed:

```plenty
def first(values: &list[i64]) -> i64:
    values[0]

def add(values: &mut list[i64], value: i64) -> Result[(), AllocError]:
    values.append(value)?
    Ok(())

def main() -> Result[(), Failure]:
    mut numbers = [1, 2]?
    print(first(numbers))?
    add(&mut numbers, 3)?
    print(numbers)?
    Ok(())
```

```output
1
[1, 2, 3]
```

References can also be local bindings. `&mut` grants access to the target; it
does not make the binding itself reassignable. Use `*reference` to read a scalar or replace
the target. Collection operations such as `append`, indexing, and `len` work
through references directly.

The parameter determines borrowing: `first(numbers)` implicitly lends `numbers`
because `first` expects `&list[i64]`. Writing `first(&numbers)` also works.
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


A shared loan permits simultaneous observation. An exclusive loan prevents
conflicting access until its last use. Collection operations work directly
through references; `*reference` reads or replaces a scalar target.
