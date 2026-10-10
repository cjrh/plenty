# Use values through a box

Call collection and string methods through a box, including nested boxes.
Methods borrow its content; mutation requires a `mut` binding or an exclusive
reference:

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

Read-only methods also work on temporary boxes. Bind a box with `mut` before
calling a mutating method.

The [recursive-data lesson](recursive-data.md) explains when a box transfers
its contents and when it keeps ownership of them.
