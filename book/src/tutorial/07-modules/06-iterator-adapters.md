# Transform a stream of values

`std.iter` has functions that take a generator and a callback. `map` and
`filter` return a new generator; `fold` reduces one to a single value.

```plenty
from std.iter import map, filter, fold

def numbers(stop: i64) -> Generator[i64]:
    for n in range(stop):
        yield n

def even(n: &i64) -> bool:
    *n % 2 == 0

def square(n: i64) -> i64:
    n * n

def add(total: i64, n: i64) -> i64:
    total + n

def main() -> Result[(), AllocError | IoError]:
    for n in map(filter(numbers(10), even), square):
        print(n)?
        if n > 3:
            break
    values = [3, 4, 5]?
    print(fold(map((n for n in values), square), 0, add))?
    Ok(())
```

```output
0
4
50
```

Nothing runs until the loop asks for a value, and `break` drops the rest of
the chain: `numbers` never makes 3. Each adapter owns its source and callback
and allocates nothing.

A source must be a generator. `(n for n in values)` turns a list into one and
takes ownership of the list. A closure passed as a callback must own its
captures:

```plenty
from std.iter import map, take

def naturals() -> Generator[i64]:
    mut n = 0
    while True:
        yield n
        n += 1

def main() -> Result[(), AllocError | IoError]:
    offset = 100
    for n in take(map(naturals(), def [offset](n: i64) -> i64: n + offset), 3):
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
[the module reference](../../design/stdlib/iter.md).
