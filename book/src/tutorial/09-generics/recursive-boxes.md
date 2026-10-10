# Use values through a box

Collection and string methods also reach through boxes, including nested boxes.
The call borrows the content, so the box remains usable. Mutating a collection
requires a `mut` binding or an exclusive reference:

```plenty
def main() -> Result[(), Failure]:
    mut items = Box(Box([1, 2]?)?)?
    items.append(3)?
    print(items.get(2).unwrap())?
    text = Box("  hello  ")?
    print(text.strip()?)?
    print(text.startswith("  "))?
    Ok(())
```

```output
3
hello
True
```

A shared reference can call reading methods such as `get` and `strip`, but
cannot call mutations such as `append` or `clear`. Read-only methods also work
on temporary boxes; bind a box with `mut` before calling a mutating method.


The [recursive-data lesson](recursive-data.md) explains when a box transfers
its contents and when it keeps ownership of them.
