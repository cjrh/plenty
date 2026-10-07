# Passing and returning generators

Use `Generator[T]` for a parameter or return annotation. The compiler keeps track
of the concrete producer and its suspended state:

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

`total` accepts either producer. The compiler specializes it for each concrete
frame type without boxing the generator. Passing a generator transfers ownership;
use `&mut Generator[T]` when a helper should advance an existing owner with `next`.
Aliases and `Option`/`Result` wrappers preserve the concrete frame type too.

A factory must return the same concrete producer on all paths in one
specialization. Matching yielded types alone is insufficient:

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

For this case, put the branch inside one generator body and yield the desired
values there. A generator may contain another generator, but recursive inline
frames are rejected because their storage would have no finite size. Generator
frames allocate nothing; allocations performed by their arguments and bodies
still use the normal fallible APIs.
