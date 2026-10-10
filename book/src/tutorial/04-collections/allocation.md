# Handle allocation failures

A collection needs storage for its mutable contents. Construction returns
`Result[collection, AllocError]`, letting the caller recover when storage
cannot be obtained. Use the same `match` and `?` rules as for other results.

```plenty
def answers() -> Result[list[i64], AllocError]:
    mut values = list[i64]()?
    values.append(21)?
    values.append(42)?
    Ok(values)

def main() -> Result[(), IoError]:
    match answers():
        case Ok(values):
            print(values)?
        case Err(error):
            match error:
                case AllocError.OutOfMemory:
                    print("not enough memory")?
                case AllocError.CapacityOverflow:
                    print("requested capacity is too large")?
    Ok(())
```

```output
[21, 42]
```

`list[i64]()` constructs an empty list. `dict[str, i64]()` and `set[str]()`
construct the other collection kinds. Displays such as `[21, 42]` return the
same kind of allocation result; extract their collection with `?` too.

Growth also returns a result: `append` for lists, `insert` for dictionaries,
and `add` for sets. A failed insertion keeps existing contents but consumes
its arguments, cleaning up any owned argument rather than returning it.

`AllocError` has two payload-free variants. `OutOfMemory` means the
allocator rejected a request. `CapacityOverflow` means its size cannot be
represented. The error itself needs no allocation.

[Capacity and reservation examples](../../design/examples/collections/capacity.md)
show how to reserve storage before inserting or deliberately recover from a
capacity error. Allocating literals, collection constructors, comprehensions,
string-building operations, and `copy` all expose results. An empty display
such as `[]` also needs an element type from context and result handling.
