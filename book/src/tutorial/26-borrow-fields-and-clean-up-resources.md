# 26. Borrow fields and clean up resources

Fields can be borrowed directly, including fields inside nested classes:

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

The borrow ends after `count`'s last use. Distinct fields can be borrowed or
changed independently. Borrowing a whole record still overlaps all its fields.
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

Use `__del__` for cleanup that belongs to an owned instance. It runs automatically
when the instance leaves scope, is replaced, or is explicitly dropped. Its bare
`self` is an exclusive reference, like in `__init__`:

```plenty
class Resource:
    name: str

    def __del__(self) -> ():
        print(("release " + self.name).unwrap()).unwrap()

class Pair:
    first: Resource
    second: Resource

    def __del__(self) -> ():
        print("release pair").unwrap()

def work() -> Result[(), Failure]:
    pair = Pair(Resource("first"), Resource("second"))
    spare = Resource("spare")
    print("working")?
    Ok(())

def main() -> Result[(), Failure]:
    work()?
    print("done")?
    Ok(())
```

```output
working
release spare
release pair
release first
release second
done
```

Locals clean up in reverse declaration order. A class's `__del__` runs before
automatic field cleanup; fields then clean up in declaration order. You do not
need to destroy the fields yourself. Moving an instance transfers its cleanup
responsibility and does not run the destructor.

Lifecycle methods are not called directly. A class with custom cleanup, or one
containing a value with custom cleanup, cannot be copied yet. That avoids
duplicating ownership of a resource accidentally. A destructor cannot yield or
return a recoverable error; an explicit closing method could return `Result`
when reporting failure matters. Fatal traps do not run cleanup.

Functions holding values with observable cleanup currently use ordinary calls
in return position so that cleanup still happens after the called function.
Numeric tail-recursive functions from the earlier lesson keep their tail calls.
