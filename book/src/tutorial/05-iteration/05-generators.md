# Produce values incrementally

A generator function declares `Generator[T]` and uses `yield` statements:

```plenty
def countdown(start: i64) -> Generator[i64]:
    print("starting").unwrap()
    mut remaining = start
    while remaining > 0:
        yield remaining
        remaining = remaining - 1

def main() -> Result[(), Failure]:
    numbers = countdown(3)
    print("created")?
    for number in numbers:
        print(number)?
    Ok(())
```

```output
created
starting
3
2
1
```

Construction returns the generator directly, with no heap allocation or `?`.
Calling the function evaluates its arguments immediately, but its body starts
only when iteration requests the first value. Each yield pauses the body and
keeps its locals for the next request. A bare return or the end of the body
finishes the generator. It cannot return a value. Allocating expressions in its
arguments or body still require their usual error handling.

Comprehensions and collection constructors also consume generators:

```plenty
def numbers(limit: i64) -> Generator[i64]:
    for n in range(limit):
        yield n

def main() -> Result[(), Failure]:
    print([n * n for n in numbers(6) if n % 2 == 1]?)?
    print(list(numbers(3))?)?
    Ok(())
```

```output
[1, 9, 25]
[0, 1, 2]
```

For one value at a time, name a mutable generator and call `next(&mut messages)`.
The `&mut` lends exclusive access without consuming the generator; the shorthand
`next(messages)` also works.
It returns `Some(value)` or `Nothing`; exhaustion stays exhausted:

```plenty
def once() -> Generator[str]:
    yield "hello"

def main() -> Result[(), Failure]:
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

Breaking out of consuming iteration drops the suspended generator without
executing the statements after its last yield. There are no generator expressions,
`yield from`, `send`, or async operations. Yielded owned values transfer ownership; use `yield copy(value)` to retain an
independent mutable value in the generator (handle the copy's `Result` first).
A generator may yield strings,
collections, classes, and enums, but not another generator.
