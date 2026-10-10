# Clean up owned instances

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

A field's children also finish cleanup before the next field begins, including
when the fields mix collections, boxes, and inline instances:

```plenty
class Leaf:
    name: str
    def __del__(self) -> ():
        print(self.name).unwrap()
class Pair:
    first: list[Leaf]
    second: Leaf
def main() -> ():
    pairs = [Pair([Leaf("a")].unwrap(), Leaf("b"))].unwrap()
    drop(pairs)
```

```output
a
b
```

Lifecycle methods are not called directly. A class with custom cleanup, or one
containing a value with custom cleanup, cannot be copied yet. That avoids
duplicating ownership of a resource accidentally. A destructor cannot yield or
return a recoverable error; an explicit closing method could return `Result`
when reporting failure matters. Fatal traps do not run cleanup.
