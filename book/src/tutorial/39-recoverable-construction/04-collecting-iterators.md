# Collecting iterators

Comprehensions support the same explicit allocation boundary:

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

Output allocation failure stops iteration and drops the partial result. It does
not undo earlier effects or catch failures in ordinary calls inside the expression.

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

`from` consumes the source. If output allocation fails, it drops the partial
list and remaining iterator. Earlier iterator side effects are not undone, and
allocations inside the generator body still follow that body's chosen APIs.

Sets provide the same constructor, removing duplicates in first-seen order:

```plenty
def main() -> Result[(), Failure]:
    print(set[i64].from([3, 1, 3, 2]?)?)?
    Ok(())
```
```output
{3, 1, 2}
```

The example propagates input construction failure with `[3, 1, 3, 2]?`.
Dictionary sources iterate
over keys; borrowed sources and string iteration are not supported by `from`.
