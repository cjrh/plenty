# Collect results into owned collections

Return a comprehension's `Result` directly when the caller should handle
allocation failure:

```plenty
def squares() -> Result[list[i64], AllocError]:
    [n * n for n in range(6) if n % 2 == 0]

def main() -> Result[(), Failure]:
    print(squares()?)?
    Ok(())
```
```output
[0, 4, 16]
```

Collect an owned iterator with recoverable list growth:

```plenty
def numbers() -> Generator[i64]:
    yield 3
    yield 6

def collect() -> Result[list[i64], AllocError]:
    source = numbers()
    list[i64].from(source)

def main() -> Result[(), Failure]:
    print(collect()?)?
    Ok(())
```
```output
[3, 6]
```

`from` consumes the source. If allocation fails, it drops the partial list and
remaining iterator. Neither `from` nor comprehensions undo earlier effects or
handle failures inside the producer's body.

Sets provide the same constructor, removing duplicates in first-seen order:

```plenty
def main() -> Result[(), Failure]:
    print(set[i64].from([3, 1, 3, 2]?)?)?
    Ok(())
```
```output
{3, 1, 2}
```

Dictionary sources iterate over keys. `from` does not support borrowed sources
or string iteration.
