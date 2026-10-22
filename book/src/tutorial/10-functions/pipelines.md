# Reduce a borrowed collection with a pipeline

Use `iter` to compose local transformations without making an intermediate
collection. Borrow the source when the caller should keep it.

```plenty
class Payment:
    name: str
    cents: i64

def add(total: i64, cents: i64) -> i64:
    total + cents

def main() -> Result[(), AllocError | IoError]:
    payments = [Payment("bob", 120), Payment("ada", 300), Payment("bob", 80)]?
    selected = iter(&payments).filter(def(p: &Payment) -> bool: p.name == "bob")
    total = selected.map(def(p: &Payment) -> i64: p.cents).fold(0, add)
    print(total, len(payments))?
    Ok(())
```
```output
200 3
```

`selected` holds a loan of `payments`. The filter and map run when `fold`
consumes the pipeline, in source order. `fold` starts at zero and passes the
accumulator and each amount to `add`. The payments remain available afterwards.
Trying to consume `selected` again is an ownership error.

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

Keep a plain loop for several statements or error propagation, and use a
comprehension when building a list is the clearest form. Pipelines are local:
return an owned collection or generator when values must cross a function
boundary. See the [pipeline reference](../../design/language/09-iteration/06-pipelines.md)
for terminals and the current limits.
