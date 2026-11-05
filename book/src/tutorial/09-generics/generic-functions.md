# Replace duplicated functions with a type parameter

Suppose the same operation is needed for two integer widths. Separate
functions work, but repeat both the implementation and its interface:

```plenty
def keep_small(value: u8) -> u8:
    value

def keep_large(value: i64) -> i64:
    value

def main() -> Result[(), IoError]:
    print(keep_small(7u8))?
    print(keep_large(9))?
    Ok(())
```
```output
7
9
```

A type parameter lets one declaration accept either width while keeping the
argument and result types the same.

```plenty
def identity[T](value: T) -> T:
    value

def sum_to[T: IntType](stop: T) -> Result[T, OverflowError[T]]:
    mut total: T = 0
    for n in range[T](stop):
        total = total + n
    Ok(total)

def first[T](values: &list[T]) -> &T:
    &values[0]

def main() -> Result[(), AllocError | IoError | OverflowError[u16]]:
    print(sum_to(5u16)?)?
    values = identity([3, 4]?)
    print(first(&values))?
    print(values)?
    Ok(())
```
```output
10
3
[3, 4]
```

Declare type parameters after the function name. Calls infer them from the
argument types, or you can supply them explicitly before the call.
Ownership still follows the concrete type: `identity(values)` transfers the
list, while a function taking `&T` borrows its argument.

`IntType` restricts a parameter to integer types, and `FloatType` to `f32` and
`f64`. `CopyType` accepts any type whose values copy, such as numbers, `str`,
and tuples of them, and rejects lists and other values that move; see
[`CopyType`](../../design/language/07-generics/index.md#copytype). Unconstrained `T` is useful
when the body only moves, borrows, or uses operations supported by the chosen
concrete type. Each specialization is checked against those operations.

A type parameter can also stand for unit `()` inside a `Result` or `Option`.
This lets one helper handle both operations that return a value and operations
that only succeed or fail:

```plenty
def keep_result[T, E](result: Result[T, E]) -> Result[T, E]:
    result

def main() -> Result[(), AllocError | IoError]:
    written = print("saved")
    keep_result(written)?
    Ok(())
```
```output
saved
```

Here `T` is inferred as `()`. You can also write an explicit unit type argument,
such as `keep_result[(), IoError](written)`. Unit still cannot be a direct
function parameter, so `identity[()](())` is rejected.

## Require collection keys

Use `K: KeyType` when a generic algorithm needs dictionary keys or set members.
It accepts integers, booleans, strings, bytes, and supported tuples, classes, enums and anonymous sums, including custom cleanup. The compiler checks all enum payloads; no hashing method is required.

```plenty
enum Label:
    Id(i64)
    Name(str)

def frequencies[K: KeyType](values: list[K]) -> Result[dict[K, i64], AllocError | OverflowError[i64]]:
    mut counts: dict[K, i64] = {}?
    for value in values:
        count = counts.get(&value, 0) + 1
        counts.insert(value, count)?
    Ok(counts)

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    values = [Label.Id(7), Label.Name("7"), Label.Id(7)]?
    counts = frequencies(values)?
    print(counts[Label.Id(7)], counts[Label.Name("7")])?
    Ok(())
```
```output
2 1
```

The loop consumes the list and transfers each key after borrowing it for
lookup. This also works for keys with custom cleanup, which cannot be copied.
Dictionary insertion and the count's addition remain fallible. Floats do not satisfy `KeyType`; see the
[constraint reference](../../design/language/07-generics/index.md#keytype).
