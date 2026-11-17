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

The [`std.alloc`](../../design/stdlib/alloc.md) API lets you select a system or
custom allocator for a list. The list keeps that selection for growth and
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

## Register a custom provider

A provider supplies backing blocks and receives them back for reuse or disposal.
This minimal forwarding policy delegates to a separate backing handle. Its public
methods satisfy `alloc.Provider`; no raw pointers or private hooks are needed.
The bootstrap pays for registration, while `backing` supplies later requests:

```plenty
import std.alloc as alloc

pub class Forwarding:
    backing: alloc.Allocator

    pub def allocate(self: &mut Forwarding, request: &alloc.Layout) -> Result[alloc.ByteBlock, AllocError]:
        self.backing.block(request)

    pub def release(self: &mut Forwarding, storage: alloc.ByteBlock) -> ():
        drop(storage)

def main() -> Result[(), alloc.RegistrationError[Forwarding] | AllocError | IoError]:
    bootstrap = alloc.system()
    backing = alloc.system()
    owner = alloc.allocator(&bootstrap, Forwarding(backing))?
    mut values = owner.list_with_capacity[i64](2)?
    drop(bootstrap)
    drop(owner)
    values.append(21)?
    values.append(42)?
    print(values)?
    Ok(())
```
```output
[21, 42]
```

Use the system handle directly when forwarding adds no policy; this example
isolates the provider contract rather than offering an allocation optimization.
Registration makes two allocations through the explicit bootstrap (state box
and control allocation), with no hidden fallback. Either failure returns the
unchanged provider in `RegistrationError.Allocation`; `?` preserves that complete
error here. A native unsupported context returns `UnsupportedContext` with the
provider. Success lets lists and blocks outlive both initiating handles.
Pre-existing allocations inside the provider are not redirected.

The request includes private metadata and padding. `release` receives the full
original backing with all bytes zeroed, so a caching policy can safely reuse it.
Callbacks serialize, and the compiler checks worker-safe state, effects and
cleanup even without `spawn`. Return allocation errors rather than trapping:
callback traps, including cleanup traps, terminate the process.

## Current limits

Retained allocator operations cannot coexist with actor operations anywhere in
the lowered program, including unused imports and layout-only use. Selection
does not redirect allocations made by element expressions, copies or other
standard-library operations, and ordinary list element restrictions still apply.
Public selected boxes and generator collection are not available. Ordinary
constructors remain available during the explicit-allocation migration; this
milestone does not remove implicit defaults or provide resettable arenas.

Use [capacity and reservation](../../design/examples/collections/capacity.md)
to obtain storage before inserting values you want to keep if reservation
fails. For byte-oriented allocator policies, see the safe-block section of
[`std.alloc`](../../design/stdlib/alloc.md#safe-byte-blocks).
