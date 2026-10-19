# Clean up owned instances

Use `__del__` for cleanup that belongs to an owned instance. It runs automatically
when the instance leaves scope, is replaced, or is explicitly dropped. Its bare
`self` is an exclusive reference, like in `__init__`:

```plenty
class Resource:
    name: str

    def __del__(self) -> ():
        drop(print("release", self.name))

class Pair:
    first: Resource
    second: Resource

    def __del__(self) -> ():
        drop(print("release pair"))

def work() -> Result[(), IoError]:
    pair = Pair(Resource("first"), Resource("second"))
    spare = Resource("spare")
    print("working")?
    Ok(())

def main() -> Result[(), IoError]:
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

Locals clean up in reverse declaration order. A class's `__del__` runs first;
its fields then clean up automatically in declaration order. Moving an instance
transfers cleanup responsibility without running the destructor.

Each field finishes cleaning up its children before the next field begins:

```plenty
class Leaf:
    name: str
    def __del__(self) -> ():
        drop(print(self.name))
class Pair:
    first: list[Leaf]
    second: Leaf
def main() -> Result[(), AllocError]:
    pairs = [Pair([Leaf("a")]?, Leaf("b"))]?
    drop(pairs)
    Ok(())
```

```output
a
b
```

Lifecycle methods are not called directly. A class with custom cleanup, or one
containing a value with custom cleanup, cannot be copied yet. A destructor cannot
yield or report a recoverable error; use an explicit closing method returning
`Result` when failures matter. Fatal traps do not run cleanup.

The lifecycle trace uses `drop(print(...))` deliberately: cleanup cannot return
an error, and a failed diagnostic write must not interrupt cleanup. For output
that must succeed, provide an explicit fallible operation and propagate its
result before leaving the scope.
