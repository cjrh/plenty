# Build recursive data

A class or enum can contain more values of its own type. This is useful for
chains, trees, and syntax nodes. Ordinary values are stored inline, so a type
that contained itself directly would have no finite size. Store the recursive
part in a `Box`, which owns one value on the heap, or in a list, set, or dict.

This chain ends with `Empty`. Each `Link` owns a value and a box holding the
rest of the chain:

```plenty
enum Chain[T]:
    Empty
    Link(T, Box[Chain[T]])

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
    mut chain = Chain[i64].Empty
    for value in range(1, 4):
        chain = Chain[i64].Link(value, Box(chain)?)
    print(total(chain))?
    Ok(())
```

```output
6
```

`Box(value)` moves the value onto the heap. That allocation can fail, so it
returns a `Result` and uses the ordinary `?` propagation. Constructing the
`Link` itself allocates nothing.

Taking a value back out of a box allocates nothing and cannot fail, so it needs
no syntax. `rest` is a `Box[Chain[i64]]` and `remaining` is a `Chain[i64]`, so
`remaining = rest` moves the content out and frees the box. The same conversion
happens wherever the content's type is required: an argument, a return value,
an annotated binding, an assignment, a constructor field, or a collection
element. `total` consumes the chain: each match transfers the tail into `rest`,
then the loop puts its content back into `remaining`. The caller cannot use
`chain` after that call.

A boxed class's fields and methods are reached as `b.field` and `b.method()`,
and where a function expects `&T`, you can pass a `&Box[T]`. Where no type is
required, a box stays a box: `other = b` moves the box itself. Operators and
conditions do not convert either. Write `*b` there to move the content out, or
`&*b` and `&mut *b` to borrow it.

Without the box, the compiler rejects the declaration:

```plenty-error
enum Chain:
    Empty
    Link(i64, Chain)

def main() -> ():
    pass
```

```error
`Chain` contains itself and would have infinite size; store the recursive field in a `Box`, list, set, or dict
```

Classes can own recursive children too. A list of children already keeps them
on the heap, so it needs no box. This companion module declares a generic tree
node with public fields:

```plenty-file forest.plenty
pub class Node[T]:
    pub value: T
    pub children: list[Node[T]]
```

```plenty
import forest

def main() -> Result[(), Failure]:
    mut root = forest.Node[i64](1, [forest.Node[i64](2, []?)]?)
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
child, a field such as `next: Option[Box[Node]]` can terminate with `Nothing`.
Classes and enums may also refer to one another through aliases.

To take an owned child out of a mutable class, leave a replacement behind.
`replace(node.next, Nothing)` returns the old tail without cloning or allocating:

```plenty
class Link:
    value: i64
    next: Option[Box[Link]]

def main() -> Result[(), Failure]:
    mut chain = Some(Box(Link(1, Some(Box(Link(2, Nothing))?)))?)
    while True:
        match chain:
            case Some(link):
                mut node = link
                print(node.value)?
                chain = replace(node.next, Nothing)
            case Nothing:
                break
    Ok(())
```

```output
1
2
```

The old tail becomes `chain`; dropping `node` now sees an empty `next` field.
Installing another tail instead of `Nothing` also lets you relink existing nodes.
`replace` requires a mutable field and returns its previous value directly.
It evaluates the replacement before any destination indices, just like assignment.

Leaving scope drops the whole owned structure automatically. Automatic cleanup
uses an allocation-free queue for boxes, so its native stack usage does not grow
with the number of nodes in a chain. If you write a recursive traversal function
yourself, ordinary function-call stack limits still apply; the loop above avoids
that.

To inspect a structure while keeping it, [match a borrowed value](78-match-borrowed-values.md).
Moving individual owned fields out without replacing them remains unsupported.
Automatic `copy`, equality, `print`, and `str.repr` also reject recursive data,
including containers holding it. Print selected scalar fields as above, or write
an explicit traversal. These restrictions prevent the existing recursive runtime
operations from overflowing the stack on otherwise valid deep values.
