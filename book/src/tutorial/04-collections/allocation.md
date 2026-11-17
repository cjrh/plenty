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

## Select list storage and backing blocks explicitly

The initial [`std.alloc`](../../design/stdlib/alloc.md) API lets you select the
system allocator for a list. The list keeps that selection for growth and
cleanup, even after the handle is dropped. The same allocator can also provide
affine zero-initialized byte blocks:

```plenty
import std.alloc as alloc

def main() -> Result[(), AllocError | IoError]:
    owner = alloc.system()
    mut values = owner.list_with_capacity[i64](2)?
    drop(owner)
    values.append(21)?
    values.append(42)?
    print(values)?
    Ok(())
```
```output
[21, 42]
```

## Allocate backing blocks

For allocator backing storage, `layout` validates a byte size and alignment
without allocating. `block` allocates zeroed bytes; even zero-size blocks allocate
metadata. Use [`Bytes`](../../design/stdlib/bytes.md) for ordinary byte I/O.

```plenty
import std.alloc as alloc

def main() -> Result[(), alloc.LayoutError | AllocError | IoError]:
    owner = alloc.system()
    request = alloc.layout(10)?
    mut block = owner.block(&request)?
    mut tail = block.split_off(5)
    drop(owner)
    tail.set(0, 42u8)
    print(block.len(), block.get(0), tail.get(0))?
    Ok(())
```
```output
5 0 42
```

Splitting transfers disjoint ranges without allocation or copying. Each range,
including an empty one, retains the full backing until the last range is dropped.
Blocks can be borrowed for reading, but cannot be copied into overlapping owners.
This API currently offers only system-root allocation for blocks and builtin
lists. It cannot coexist with actor operations, and does not redirect
allocations made by element expressions, copies or other standard-library
operations. The ordinary constructors above remain available during the
explicit-allocation migration.

Use [capacity and reservation](../../design/examples/collections/capacity.md)
to obtain storage before inserting values you want to keep if reservation
fails. For byte-oriented allocator policies, see the safe-block section of
[`std.alloc`](../../design/stdlib/alloc.md#safe-byte-blocks).
