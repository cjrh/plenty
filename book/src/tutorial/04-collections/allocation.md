# Handle allocation failures

Collection construction returns `Result[collection, AllocError]`. Use `?` to
propagate a failure, or `match` to recover:

```plenty
def answers() -> Result[list[i64], AllocError]:
    mut values = list[i64].with_capacity(2)?
    values.append(21)?
    values.append(42)?
    Ok(values)

def main() -> Result[(), IoError]:
    match answers():
        case Ok(values):
            print(values)?
        case Err(AllocError.OutOfMemory):
            print("not enough memory")?
        case Err(AllocError.CapacityOverflow):
            print("requested capacity is too large")?
    Ok(())
```

```output
[21, 42]
```

`list[i64].with_capacity(2)?` constructs an empty list with room for at least
two elements. Appending those elements needs no further list storage allocation.
Use `list[i64]()` when you do not know the capacity ahead of time.
`dict[str, i64]()` and `set[str]()`
construct the other collection kinds. Displays such as `[21, 42]` return the
same kind of allocation result; extract their collection with `?` too.
List displays reserve their full element count automatically. Both mutable and
immutable list bindings benefit. Element expressions may still allocate.

For an existing mutable collection, `values.reserve(n)?` reserves room for
`n` **additional** elements beyond its current length. Existing contents remain
unchanged if reservation fails.

Growth also returns a result: `append` for lists, `insert` for dictionaries,
and `add` for sets. A failed insertion keeps existing contents but consumes
its arguments, cleaning up any owned argument rather than returning it.

`AllocError` has two payload-free variants. `OutOfMemory` means the
allocator rejected a request. `CapacityOverflow` means its size cannot be
represented. Neither variant needs allocation.

Use [capacity and reservation](../../design/examples/collections/capacity.md)
to obtain storage before inserting values you want to keep if reservation
fails.
