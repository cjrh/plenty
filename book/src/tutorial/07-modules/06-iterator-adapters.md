# Transform a stream of values

Generators have fluent `map` and `filter` methods that build a lazy pipeline;
`fold` reduces it to a single value. Adapters fuse into the consuming loop.
For a collection, start with an [`iter` pipeline](../10-functions/pipelines.md),
which can borrow the collection directly.

```plenty
def numbers(stop: i64) -> Generator[i64]:
    for n in range(stop):
        yield n

def even(n: &i64) -> bool:
    *n % 2 == 0

def square(n: i64) -> i64:
    n * n

def square_borrowed(n: &i64) -> i64:
    *n * *n

def add(total: i64, n: i64) -> i64:
    total + n

def main() -> Result[(), AllocError | IoError]:
    for n in numbers(10).filter(even).map(square):
        print(n)?
        if n > 3:
            break
    values = [3, 4, 5]?
    print(iter(&values).map(square_borrowed).fold(0, add))?
    Ok(())
```

```output
0
4
50
```

The generator body and callbacks run only when the loop asks for a value, and
`break` drops the rest of the chain: `numbers` never makes 3. Source expressions
and callback captures are prepared when the chain is constructed. Adapters
add no allocation or intermediate generator frame.

Both generator and collection sources accept owned callback captures. Local
pipelines can also borrow captures, as the later
[pipeline lesson](../10-functions/pipelines.md) demonstrates:

```plenty
def naturals() -> Generator[i64]:
    mut n = 0
    while True:
        yield n
        n += 1

def main() -> Result[(), IoError]:
    offset = 100
    for n in naturals().map(def [offset](n: i64) -> i64: n + offset).take(3):
        print(n)?
    Ok(())
```

```output
100
101
102
```

`take` makes an endless generator finite. `try_fold` is `fold` for a callback
that returns `Result`; it stops at the first error. See
[the pipeline reference](../../design/language/09-iteration/06-pipelines.md).

When a chain needs to leave its function, end it with `.generator()`. This
explicitly transfers owned state into one resumable frame while keeping the
adapters fused inside it:

```plenty
def squares(stop: i64) -> Generator[i64]:
    iter(range(stop)).map(def(n: i64) -> i64: n * n).generator()

def total(values: Generator[i64]) -> i64:
    values.fold(0, def(acc: i64, n: i64) -> i64: acc + n)

def main() -> Result[(), IoError]:
    print(total(squares(4)))?
    mut values = squares(3)
    print(next(values), next(values))?
    Ok(())
```
```output
14
Option[i64].Some(0) Option[i64].Some(1)
```

Conversion does not visit any items. The returned generator owns its source and
captures, so borrowed lists or borrowed captures cannot be converted, even
after mapping to owned values. Dropping it early releases its remaining state.
Use local consumption when you do not need a resumable stream: `.generator()`
adds a suspension boundary. The older [`std.iter` functions](../../design/stdlib/iter.md)
remain available for compatibility.
