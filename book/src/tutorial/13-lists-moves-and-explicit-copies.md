# 13. Lists, moves, and explicit copies

A list contains values of one type. Use `list[T]` in signatures and annotations.
An empty list needs an annotation or a typed constructor such as `list[i64]()`.

```plenty
def main() -> Result[(), Failure]:
    mut original: list[i64] = [10, 20]?
    mut changed = copy(original)?
    changed.append(30)?
    changed[0] = 99
    print(original)?
    print(changed)?
    print(changed[-1])?
    print(len(changed))?
    Ok(())
```

```output
[10, 20]
[99, 20, 30]
30
3
```

The explicit `copy(original)` creates independent contents. Without `copy`,
assignment transfers ownership and the old binding cannot be used. Updates happen
in place and require a `mut` owner or an exclusive reference (lesson 22).
Owned function parameters are immutable bindings; reference parameters can grant
permission to change the caller's value.
Negative indices count from the end. Invalid indices stop the program with a
runtime error.

`copy(original)` returns a `Result`, as do all allocating operations. The
examples here propagate failures with `?`; lesson 19 shows how to recover instead.

Collections can nest. To update an inner list while keeping the outer collection,
use `mut child = copy(rows[0])?`, update `child`, then assign it back with
`rows[0] = child`. This last assignment transfers the child into the collection. Direct nested
assignment such as `rows[0][0] = 1` is not implemented.
