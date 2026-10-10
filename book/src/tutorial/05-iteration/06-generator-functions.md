# Passing and returning generators

Use `Generator[T]` to accept or return a generator:

```plenty
def once(n: i64) -> Generator[i64]:
    yield n

def pair() -> Generator[i64]:
    yield 10
    yield 20

def source(n: i64) -> Generator[i64]:
    once(n)

def total(values: Generator[i64]) -> i64:
    mut result = 0
    for value in values:
        result = result + value
    result

def main() -> Result[(), IoError]:
    print(total(source(7)))?
    print(total(pair()))?
    Ok(())
```
```output
7
30
```

`total` accepts either producer. Passing a generator transfers ownership; use
`&mut Generator[T]` when a helper should advance an existing owner with `next`.

A factory must return the same producer on every path. Two generators yielding
the same type are not interchangeable return values:

```plenty-error
def one() -> Generator[i64]:
    yield 1

def two() -> Generator[i64]:
    yield 2
    yield 3

def choose(flag: bool) -> Generator[i64]:
    if flag:
        return one()
    two()

def main() -> ():
    drop(choose(True))
```
```error
expected Generator[i64] from `one`, got Generator[i64] from `two`
```

Put such a branch inside one generator body instead. A generator may contain
another generator, but recursive generator state is rejected: its inline storage
would have no finite size.
