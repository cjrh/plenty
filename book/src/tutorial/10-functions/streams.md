# Transform a stream of values

A generator has the same pipeline methods as `iter(...)`, so a stream of values
can be filtered, mapped and reduced without an intermediate list:

```plenty
def numbers(stop: i64) -> Generator[i64]:
    for n in range(stop):
        yield n

def main() -> Result[(), IoError]:
    evens = numbers(10).filter(def(n: &i64) -> bool: *n % 2 == 0)
    for n in evens.map(def(n: i64) -> i64: n * n):
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

`take` makes an endless generator finite. Callbacks can capture values, as with
any closure:

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

## Return a stream

A pipeline stays in the function that builds it. When a chain needs to be
returned or passed to another function, end it with `.generator()`. This
explicitly moves the source and captures into one resumable generator, with the
adapters still fused inside it:

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
captures, so a pipeline over a borrowed list or with borrowed captures cannot be
converted, even after mapping to owned values; return an owned collection
instead. Dropping the generator early releases its remaining state.

Each `next` resumes the generator, which costs more than a fused local loop.
Consume a pipeline where it is built unless a resumable stream must cross a
function boundary. See [the pipeline reference](../../design/language/09-iteration/06-pipelines.md)
for the full list of adapters and terminals.
