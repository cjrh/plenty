# Build recursive data

Chains and trees contain values of their own type. A directly embedded value
would have no finite size, so put the recursive field in a `Box` or collection.
A box owns one value on the heap.

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

`Box(value)` moves the value onto the heap and returns an allocation result.
Constructing the `Link` itself allocates nothing.

When a value of type `T` is required, a `Box[T]` moves its content out and frees
the box, without allocating. Here, `remaining = rest` expects `Chain[i64]`, so
it unboxes `rest`. The conversion also works for arguments, returns, annotated
bindings, constructor fields, and collection elements. `total` consumes its chain;
the caller cannot use it afterward.

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
