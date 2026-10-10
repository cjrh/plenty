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
