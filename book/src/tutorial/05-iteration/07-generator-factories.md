# Creating generators

Generator construction needs no heap allocation and returns a generator directly.
An ordinary factory can return it with the same annotation:

```plenty
def numbers() -> Generator[i64]:
    yield 10
    yield 20

def source() -> Generator[i64]:
    numbers()

def total() -> i64:
    values = source()
    mut result = 0
    for n in values:
        result = result + n
    result

def main() -> Result[(), IoError]:
    print(total())?
    Ok(())
```
```output
30
```

Call a generator function like any other function: `numbers()` creates its
suspended state without executing its body. Arguments evaluate immediately and
move into that state. Allocating arguments and operations inside the body still
use their normal `Result` APIs; collecting values into a list also allocates.

A factory that can fail for another reason can still return
`Result[Generator[T], E]`. Write `Ok(numbers())` for success and use `?` at the
factory's call site. These wrappers add no heap allocation for the frame.
Dropping a generator, including one inside `Some` or `Ok`, releases its captures
without resuming its body.
