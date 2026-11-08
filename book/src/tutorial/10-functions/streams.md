# Transform a stream of values

A generator has the same pipeline methods as `iter(...)`, so a stream of values
can be filtered, mapped and reduced without an intermediate list:

```plenty
def numbers(stop: i64) -> Generator[i64]:
    for n in range(stop):
        yield n

def main() -> Result[(), IoError | OverflowError[i64]]:
    two: NonZero[i64] = 2
    evens = numbers(10).filter(def [two](n: &i64) -> bool: *n % two == 0)
    for square in evens.map(def(n: i64) -> Result[i64, OverflowError[i64]]: Ok(n * n)):
        n = square?
        print(n)?
        if n > 3:
            break
    Ok(())
```

```output
0
4
```

The generator body and callbacks run only when the loop asks for a value, and
`break` drops the rest of the chain: `numbers` never makes 3. The adapters are
fused into the loop, so they add no allocation or generator frame of their own.

`take` limits how many items are requested from a generator. Callbacks can capture values, as with
any closure:

```plenty
def naturals() -> Generator[i64]:
    for n in range(i64.MAX):
        yield n

def main() -> Result[(), IoError | OverflowError[i64]]:
    offset = 100
    for n in naturals().map(def [offset](n: i64) -> Result[i64, OverflowError[i64]]: Ok(n + offset)).take(3):
        print(n?)?
    Ok(())
```

```output
100
101
102
```

`naturals` is bounded by the representable range rather than overflowing a
manual counter. `take(3)` requests only its first three values. The map returns
ordinary public `Result` items, which the loop propagates before printing.

## Return a stream

A pipeline stays in the function that builds it. When a chain needs to be
returned or passed to another function, end it with `.generator()`. This
explicitly moves the source and captures into one resumable generator, with the
adapters still fused inside it:

```plenty
def squares(stop: i64) -> Generator[Result[i64, OverflowError[i64]]]:
    iter(range(stop)).map(def(n: i64) -> Result[i64, OverflowError[i64]]: Ok(n * n)).generator()

def total(values: Generator[Result[i64, OverflowError[i64]]]) -> Result[i64, OverflowError[i64]]:
    values.try_fold(0, def(acc: i64, n: Result[i64, OverflowError[i64]]) -> Result[i64, OverflowError[i64]]: Ok(acc + n?))

def main() -> Result[(), IoError | OverflowError[i64]]:
    print(total(squares(4))?)?
    mut values = squares(3)
    first = next(values).unwrap()?
    second = next(values).unwrap()?
    print(Some(first), Some(second))?
    Ok(())
```
```output
14
Option[i64].Some(0) Option[i64].Some(1)
```

The two `unwrap` calls assert only that `squares(3)` has at least two items;
the arithmetic `Result` in each item is still propagated with `?`. For an
unknown-length stream, match `next` instead. A generator cannot propagate an
arithmetic error from its body: yielding `Result` items lets its consumer
decide how to handle failure. `try_fold` here handles each item as well as
its own addition.

Conversion does not visit any items. The returned generator owns its source and
captures. Dropping it early releases its remaining state.

A stream over a list the caller keeps takes the list as a reference parameter.
The generator borrows the list through that parameter, so it copies nothing:

```plenty
def evens(values: &list[i64]) -> Generator[i64]:
    iter(values).filter(def(n: &i64) -> bool: *n % 2 == 0).map(def(n: &i64) -> i64: *n).generator()

def main() -> Result[(), AllocError | IoError]:
    mut values = [1, 2, 3, 4]?
    for n in evens(&values):
        print(n)?
    values.append(6)?
    print(len(values))?
    Ok(())
```
```output
2
4
5
```

The caller's `values` stays borrowed until the stream's last use, here the end
of the loop; appending while the stream could still resume is a borrow error.
The items must be owned, so the `map` copies each `&i64` out. A stream cannot
borrow a list local to the function that returns it, and a callback that
borrows cannot be converted; return an owned collection instead.

Each `next` resumes the generator, which costs more than a fused local loop.
Consume a pipeline where it is built unless a resumable stream must cross a
function boundary. See [the pipeline reference](../../design/language/09-iteration/06-pipelines.md)
for the full list of adapters and terminals.
