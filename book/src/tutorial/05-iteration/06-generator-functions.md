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

def total(values: Generator[i64]) -> Result[i64, OverflowError[i64]]:
    mut result = 0
    for value in values:
        result = result + value
    Ok(result)

def main() -> Result[(), IoError | OverflowError[i64]]:
    print(total(source(7))?)?
    print(total(pair())?)?
    Ok(())
```
```output
7
30
```

`total` accepts either producer. Passing a generator transfers ownership.

To consume only part of a generator and keep the rest, lend it with `&mut`.
A loop over the lent generator advances the owner, so `break` or `return`
leaves the remaining values for the caller:

```plenty
def countdown(n: i64) -> Generator[i64]:
    mut value = n
    while value > 0:
        yield value
        # value > 0, so the decrement cannot overflow.
        unchecked:
            value = value - 1

def skip_to(values: &mut Generator[i64], stop: i64) -> ():
    for value in values:
        if value == stop:
            return

def main() -> Result[(), IoError]:
    mut values = countdown(5)
    skip_to(&mut values, 4)
    for value in &mut values:
        print(value)?
        if value == 2:
            break
    print(next(values))?
    Ok(())
```
```output
3
2
Option[i64].Some(1)
```
Once you know closures, [Transform a stream of values](../10-functions/streams.md)
shows how to filter, map and reduce a generator without writing the loop.

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
expected Generator[i64] from `one`, got Generator[i64] from `two`; each producer has its own inline frame, so different producers cannot join; branch inside one generator body instead
```

Put such a branch inside one generator body instead. A generator may contain
another generator, but recursive generator state is rejected: its inline storage
would have no finite size.
