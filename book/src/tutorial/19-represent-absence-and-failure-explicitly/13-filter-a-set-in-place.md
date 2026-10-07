# Filter a set in place

`intersection_update` retains common members in a mutable set. It borrows the
other set, returns unit, and reuses existing storage without allocating:

```plenty
def main() -> Result[(), Failure]:
    mut selected = {1, 2, 3}?
    allowed = {2, 3, 4}?
    selected.intersection_update(allowed)
    print(len(selected))?
    print(1 in selected)?
    print(2 in selected and 3 in selected)?
    print(len(allowed))?
    Ok(())
```

```output
2
False
True
3
```

The source must be a different set: it stays borrowed while the destination is
mutated. Use `intersection` when you want an independent result instead.

Use `difference_update` to remove the other set's members in place:

```plenty
def main() -> Result[(), Failure]:
    mut pending = {1, 2, 3}?
    done = {2, 3, 4}?
    pending.difference_update(done)
    print(len(pending))?
    print(1 in pending)?
    print(len(done))?
    Ok(())
```

```output
1
True
3
```

This also allocates nothing and keeps the source usable. To remove every member
without needing another set, use `clear()`.
