# Reverse a list in place

`reverse()` mutates a list through a `mut` binding or `&mut` reference and returns
`()`. It allocates nothing and moves no elements out of the list:

```plenty
def flip(items: &mut list[i64]) -> ():
    items.reverse()

def main() -> Result[(), Failure]:
    mut items = [10, 20, 30]?
    flip(&mut items)
    print(items)?
    items.reverse()
    print(items)?
    Ok(())
```

```output
[30, 20, 10]
[10, 20, 30]
```

Owned elements such as classes and nested lists work too. Reversing does not
copy or destroy them; eventual list cleanup follows their new order. To keep
the original order separately, explicitly copy the list first.
