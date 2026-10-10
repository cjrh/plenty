# Borrow distinct fields

Borrow a field directly with `&` or `&mut`:

```plenty
class Basket:
    count: i64
    items: list[str]

def main() -> Result[(), Failure]:
    mut basket = Basket(0, []?)
    count = &mut basket.count
    *count = 2
    basket.items.append("apple")?
    basket.items.append("pear")?
    print(basket)?
    Ok(())
```

```output
Basket(count=2, items=["apple", "pear"])
```

Distinct fields can be borrowed or changed independently. Borrowing a whole
record overlaps all its fields.
Collection element references such as `&basket.items[0]` protect the collection
against changes that could invalidate the element's address.
You can replace a class-valued field by assignment, but cannot replace a whole
class through `*reference = new_instance` yet.

```plenty
class Position:
    x: i64
    y: i64

def main() -> Result[(), Failure]:
    mut position = Position(1, 2)
    x = &mut position.x
    y = &mut position.y
    *x = 10
    *y = 20
    print(*x)?
    print(*y)?
    Ok(())
```
```output
10
20
```
