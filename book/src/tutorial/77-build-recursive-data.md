# Build recursive data

A class or enum can contain more values of its own type. This is useful for
chains, trees, and syntax nodes. Existing class and enum records provide the
indirection; you do not need a separate `Box` type.

This chain ends with `Empty`. Each `Link` owns a value and the rest of the chain:

```plenty
enum Chain[T]:
    Empty
    Link(T, Chain[T])

def total(chain: Chain[i64]) -> i64:
    mut remaining = chain
    mut answer = 0
    while True:
        match remaining:
            case Chain[i64].Empty:
                break
            case Chain[i64].Link(value, rest):
                answer = answer + value
                remaining = rest
    answer

def main() -> Result[(), Failure]:
    mut chain = Chain[i64].Empty?
    for value in range(1, 4):
        chain = Chain[i64].Link(value, chain)?
    print(total(chain))?
    Ok(())
```

```output
6
```

Constructing each user-enum record can fail, so it returns a `Result` and uses
the ordinary `?` propagation. Moving a chain or matching it adds no allocation.
`total` consumes the chain: each match transfers the tail into `rest`, then the
loop puts it back into `remaining`. The caller cannot use `chain` after that call.

Classes can own recursive children too. This companion module declares a generic
tree node with public fields:

```plenty-file forest.plenty
pub class Node[T]:
    pub value: T
    pub children: list[Node[T]]
```

```plenty
import forest

def main() -> Result[(), Failure]:
    mut root = forest.Node[i64](1, [forest.Node[i64](2, []?)?]?)?
    root.children[0].value = 9
    print(root.value)?
    print(root.children[0].value)?
    Ok(())
```

```output
1
9
```

The nested assignment updates the existing child without copying or allocating
another node. An empty children list terminates this tree. For a single optional
child, a field such as `next: Option[Node]` can terminate with `Nothing` instead;
the `Option` wrapper itself allocates nothing. Classes and enums may also refer
to one another through aliases and collections.

Leaving scope drops the whole owned structure automatically. Automatic cleanup
uses an allocation-free queue, so its native stack usage does not grow with the
number of nodes in a chain. If you write a recursive traversal function yourself,
ordinary function-call stack limits still apply; the loop above avoids that.

There are a few current limits. Matching consumes recursive enums; matching a
reference and moving individual owned fields out of a class are not supported.
Automatic `copy`, equality, `print`, and `str.repr` also reject recursive data,
including containers holding it. Print selected scalar fields as above, or write
an explicit traversal. These restrictions prevent the existing recursive runtime
operations from overflowing the stack on otherwise valid deep values.
