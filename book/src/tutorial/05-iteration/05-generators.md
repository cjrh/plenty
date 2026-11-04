# Produce values incrementally

A generator function declares `Generator[T]` and uses `yield` statements:

```plenty
def countdown(start: i64) -> Generator[i64]:
    mut remaining = start
    while remaining > 0:
        yield remaining
        # The loop proves 1 <= remaining <= i64.MAX.
        unchecked:
            remaining -= 1

def main() -> Result[(), IoError]:
    numbers = countdown(3)
    print("created")?
    for number in numbers:
        print(number)?
    Ok(())
```

```output
created
3
2
1
```

Calling a generator evaluates its arguments immediately but starts its body
only when iteration requests a value. Each `yield` pauses the body, retaining
its locals until the next request. A bare `return` or the end of the body
finishes it; a generator cannot return a value.

Creating the generator needs no allocation or `?`. Allocating arguments and
operations in its body still need their usual error handling. A generator cannot
propagate arithmetic errors to its caller. The countdown uses one small
`unchecked:` suite: the loop proves that subtracting one stays in range. For
unbounded calculations, yield `Result` items and let the consumer handle them,
as in [lazy expressions](generator-expressions.md).

Comprehensions and collection constructors also consume generators:

```plenty
def numbers(limit: i64) -> Generator[i64]:
    for n in range(limit):
        yield n

def main() -> Result[(), AllocError | IoError | ZeroDivisionError | OverflowError[i64]]:
    print([n * n for n in numbers(6) if n % 2 == 1]?)?
    print(list(numbers(3))?)?
    Ok(())
```

```output
[1, 9, 25]
[0, 1, 2]
```

Call `next(&mut messages)` to advance a mutable generator without consuming it.
The shorthand `next(messages)` also works. It returns `Some(value)` or `Nothing`;
an exhausted generator keeps returning `Nothing`:

```plenty
def once() -> Generator[str]:
    yield "hello"

def main() -> Result[(), IoError]:
    mut messages = once()
    print(next(&mut messages))?
    print(next(&mut messages))?
    print(next(&mut messages))?
    Ok(())
```

```output
Option[str].Some("hello")
Option[str].Nothing
Option[str].Nothing
```

Breaking out of consuming iteration drops the generator without resuming its
body. Yielding an owned value transfers ownership; to keep it, handle
`copy(value)`'s result and yield the copy.

Generators cannot yield other generators, references, or closures. For a lazy
expression producer, use a [generator expression](generator-expressions.md).
`yield from`, `send`, and async operations are unsupported.
