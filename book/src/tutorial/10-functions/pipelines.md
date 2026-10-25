# Reduce a borrowed collection with a pipeline

[`iter` pipelines](../05-iteration/pipelines.md) also accept functions, so
they can transform items without making an intermediate collection. Borrow the
source when the caller should keep it.

```plenty
class Payment:
    name: str
    cents: i64

def main() -> Result[(), AllocError | IoError]:
    payments = [Payment("bob", 120), Payment("ada", 300), Payment("bob", 80)]?
    selected = iter(&payments).filter(def(p: &Payment) -> bool: p.name == "bob")
    total = selected.map(def(p: &Payment) -> i64: p.cents).sum()
    largest = iter(&payments).map(def(p: &Payment) -> i64: p.cents).max()
    refund = iter(&payments).any(def(p: &Payment) -> bool: p.cents < 0)
    print(total, largest, refund, len(payments))?
    Ok(())
```
```output
200 Option[i64].Some(300) False 3
```

`selected` holds a loan of `payments`. The filter and map run when `sum`
consumes the pipeline, in source order. A closure written directly in the chain,
like the filter and map here, is compiled into that loop rather than called.
The payments remain available afterwards.
Trying to consume `selected` again is an ownership error.

`sum` and `max` work as in [Chain iteration steps with
`iter`](../05-iteration/pipelines.md#add-up-and-find-extremes); here they reduce
mapped values, since a `Payment` is neither a number nor a `str`. Likewise,
`any` and `all` take a predicate when the items are not `bool`. Use `fold` for
any other accumulation.

A method returning a borrowed list can be used directly as the source. Its
owner remains borrowed while the pipeline is in use:

```plenty
class Readings:
    values: list[i64]

    def samples(self) -> &list[i64]:
        &self.values

def main() -> Result[(), AllocError | IoError]:
    readings = Readings([10, 20, 30]?)
    product = iter(readings.samples()).fold(1, def(acc: i64, n: &i64) -> i64: acc * *n)
    print(product)?
    Ok(())
```
```output
6000
```

`fold` starts with its first argument and passes the accumulator and each item
to the callback, which returns the next accumulator.

Predicates borrow each item; `map` receives whatever the source yields. A
borrowed list yields references, so the map above takes `&Payment`. A range
yields integers by value:

```plenty
def main() -> Result[(), IoError]:
    for square in iter(range(10)).skip(2).take(3).map(def(n: i64) -> i64: n * n):
        print(square)?
    Ok(())
```
```output
4
9
16
```

Use `try_collect` when a transformation can fail and the desired result is an
owned collection. The annotation chooses the collection, and `?` preserves the
first parsing or allocation error:

```plenty
def main() -> Result[(), AllocError | ParseError | IoError]:
    lines = ["10", "20", "30"]?
    values: list[i64] = iter(&lines).map(def(line: &str) -> Result[i64, ParseError]: i64.parse(line)).try_collect()?
    print(values, len(lines))?
    Ok(())
```
```output
[10, 20, 30] 3
```

For an infallible transformation, use `collect()` with the same annotation.
`try_fold` and `try_for_each` propagate errors returned by their callbacks;
they do not automatically unwrap `Result` items.

After [`enumerate` or `zip`](../05-iteration/pipelines.md), callbacks take two
item parameters. Predicates borrow
both components; mapping receives the index by value and the borrowed item:

```plenty
def main() -> Result[(), AllocError | IoError]:
    names = ["Ada", "Bob", "Cal"]?
    indexed = iter(&names).enumerate(1)
    selected = indexed.filter(def(i: &i64, name: &str) -> bool: *i != 2)
    labels: dict[i64, str] = selected.map(def(i: i64, name: &str) -> Result[(i64, str), AllocError]: Ok((i, copy(name)?))).try_collect()?
    print(labels[1], labels[3], len(names))?
    Ok(())
```
```output
Ada Cal 3
```

Copying a name can fail, so `try_collect` propagates that error as well as
dictionary allocation errors. The successful owned pairs become dictionary entries. Repeated keys
keep the last value. A pair fold takes the accumulator followed by both item
components; a tuple accumulator stays a single parameter. Zip stops at the
shorter input, pulling the left first, so it may consume one extra left item.

Use `chain` to continue with a second source of the same item type, and
`flat_map` when each item expands into a source of its own:

```plenty
class Batch:
    first: i64
    count: i64

def main() -> Result[(), AllocError | IoError]:
    morning = [3, 1]?
    evening = [4]?
    total = iter(&morning).chain(&evening).sum()
    print(total)?
    batches = [Batch(100, 2), Batch(200, 1)]?
    for ticket in iter(&batches).flat_map(def(b: &Batch) -> range[i64]: range(b.first, b.first + b.count)):
        print(ticket)?
    Ok(())
```
```output
8
100
101
200
```

Both lists stay borrowed until the sum ends. The `flat_map` callback runs
once per batch, only after the previous batch's tickets are used up, and
returns a range, so nothing is allocated. A callback may return any owned
iterable, such as a list, range or generator; returning a list allocates one list
per item.

## Lend a pipeline whose callbacks borrow

A pipeline [lent to a function](../05-iteration/pipelines.md#lend-a-pipeline-to-a-function)
can use a closure that borrows, when the closure is written inside the
adapter call:

```plenty
def first(p: &mut Iter[i64]) -> Option[i64]:
    next(p)

def main() -> Result[(), IoError]:
    limit = 3
    mut above = iter(range(9)).filter(def [&limit](x: &i64) -> bool: *x > limit)
    print(first(&mut above))?
    print(above.count())?
    Ok(())
```
```output
Option[i64].Some(4)
4
```

Storing the same closure in a variable first and passing the variable makes
the pipeline impossible to lend:

```plenty-error
def first(p: &mut Iter[i64]) -> Option[i64]:
    next(p)

def main() -> Result[(), IoError]:
    limit = 3
    keep = def [&limit](x: &i64) -> bool:
        *x > limit
    mut above = iter(range(9)).filter(keep)
    print(first(&mut above))?
    Ok(())
```
```error
it holds closure `keep`, which is stored in a variable and borrows its captures; write the closure literal inside the adapter call instead
```

A closure written in the chain is compiled into the pipeline, so its borrow of
`limit` becomes one of the pipeline's own loans, which the caller keeps while
`first` runs. A stored closure is a value holding that reference inside it,
and a function receiving a lent pipeline cannot take such a value. Without the
lend, both forms work.

Keep a plain loop for several statements or error propagation, and use a
comprehension when building a list is the clearest form. Pipelines are local:
the next lesson applies them to generators and shows how to
[return a stream](streams.md#return-a-stream) from a function. See the
[pipeline reference](../../design/language/09-iteration/06-pipelines.md)
for terminals and the current limits.
