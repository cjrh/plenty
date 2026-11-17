# `std.alloc` — explicit retained allocators and safe byte blocks

`std.alloc` exposes the explicit system allocator and constructs ordinary
`list[T]` values whose object, metadata and row storage retain the selected
allocator. Growth and destruction use that allocator after the caller's handle
is dropped. Only the system root is publicly constructible in this initial
binding; see the [remaining B13 scope](../../backlog.md#b13-explicit-allocator-implementation-sequence).

This module also provides validated memory-layout requests and affine
zero-initialized byte blocks with checked access and disjoint splitting. These
are safe allocator backing types for custom policies: no raw pointers escape,
no copying is permitted, and blocks retain their provider through all operations.
Even empty blocks allocate metadata. Valid layouts do not guarantee the
metadata and payload can both be allocated or are available in memory.

Native runtime support is necessary for builtin storage and thread-safe retained
handles. The public wrappers are written in Plenty. The separate
[`std.experimental.alloc`](experimental-alloc.md) module remains thread-confined;
its handles and owner types are not interchangeable with these.

## Examples

### Explicit system allocator for lists

```plenty
import std.alloc as alloc

def main() -> Result[(), AllocError | IoError]:
    owner = alloc.system()
    mut values = owner.empty_list[i64]()?
    values.append(7)?
    values.append(8)?
    drop(owner)
    print(values)?
    Ok(())
```
```output
[7, 8]
```

### Safe byte blocks

```plenty
import std.alloc as alloc

def main() -> Result[(), alloc.LayoutError | AllocError | IoError]:
    owner = alloc.system()
    request = alloc.layout(10, alignment=64)?
    mut left = owner.block(&request)?
    print(left.len(), left.get(0))?
    mut right = left.split_off(5)
    drop(owner)
    left.set(4, 33u8)
    right.set(0, 44u8)
    print(left.len(), right.len(), left.get(4), right.get(0))?
    Ok(())
```
```output
10 0
5 5 33 44
```

## API

### Layout validation (allocation-free)

| Signature | Behavior |
| --- | --- |
| `layout(size: i64, alignment: i64 = 8) -> Result[Layout, LayoutError]` | Returns a validated, copyable request without allocating. Zero size is valid. Private fields prevent direct construction or mutation. |
| `Layout.size(self) -> i64` | Returns the requested size passed to `layout()`, not any rounded extent. |
| `Layout.alignment(self) -> i64` | Returns the requested alignment. |

`LayoutError` is copyable. Validation first rejects nonpositive or non-power-of-two
alignment with `InvalidAlignment`. It then rejects sizes outside
`0..=i64.MAX - (alignment - 1)` with `Allocation(AllocError.CapacityOverflow)`.
The factory never returns `OutOfMemory`. The error name distinguishes request
validation from the builtin `AllocError`; scalar alignment permits named errors
for invalid configuration rather than requiring a prevalidated integer domain.

### System allocator and lists

| Signature | Ownership, allocation and errors |
| --- | --- |
| `system() -> Allocator` | Creates an explicit system-root handle without allocation. It has no per-instance state or reference count to allocate. |
| `Allocator.share(self) -> Allocator` | Borrows the handle and returns another owning handle without allocation. For the system root there is no reference count to increment. |
| `Allocator.empty_list[T](self) -> Result[list[T], AllocError]` | Borrows the allocator and creates an empty builtin list with zero row capacity. Object and bookkeeping storage still allocate. |
| `Allocator.list_with_capacity[T](self, capacity: i64) -> Result[list[T], AllocError]` | Borrows the allocator and creates an empty builtin list with at least the requested element capacity. Zero is valid; negative or unrepresentable capacity returns `CapacityOverflow`. |

### Safe byte blocks

| Signature | Behavior |
| --- | --- |
| `Allocator.block(self, layout: &Layout) -> Result[ByteBlock, AllocError]` | Borrows both inputs and allocates exactly the requested zeroed byte range plus private metadata. Even zero size allocates. Metadata/layout overflow returns `CapacityOverflow`; unavailable storage returns `OutOfMemory`. |
| `ByteBlock.len(self) -> i64` | Returns the current range length; splitting changes it. |
| `ByteBlock.is_empty(self) -> bool` | Returns `len() == 0`. |
| `ByteBlock.alignment(self) -> i64` | Returns the actual start-address alignment of the block payload. A new block meets at least the requested alignment; splits may reduce it to one. |
| `ByteBlock.get(self, index: i64) -> u8` | Reads one byte. Index must satisfy `0 <= index < len()`, otherwise named trap. |
| `ByteBlock.set(self: &mut ByteBlock, index: i64, value: u8) -> ()` | Writes one byte. Index must satisfy `0 <= index < len()`, otherwise named trap. |
| `ByteBlock.split_off(self: &mut ByteBlock, at: i64) -> ByteBlock` | Leaves the first `at` bytes in `self` and returns the suffix without allocating or copying bytes. Requires `0 <= at <= len()`, otherwise traps. Both disjoint ranges retain the entire backing, even when empty. |

Invalid positions trap with the operation and argument, such as
`ByteBlock.get: index is outside 0..len()`. Alignment after a split describes
its new start address; splitting an aligned block at an odd offset yields
suffix alignment one.

Both list constructors return `OutOfMemory` when storage cannot be obtained.
Partial construction releases successful allocations through their selected
allocator. A failed growth leaves the existing list intact. Element types must
be heap-storable and must not hide task/channel ownership behind erasure.

`Allocator` is affine: move it or explicitly call `share()`, rather than copying
it. Its representation is private. Lists retain the allocator through element
cleanup and storage release. The native system root uses explicit system
allocation rather than ambient actor/arena routing; it has no backing state to
free on final handle drop.

`ByteBlock` cannot be directly constructed, copied or cloned; its representation
is private. Shared read borrows remain supported. Moving or dropping one range
does not invalidate other ranges from the same backing. Only the final range
releases the backing through its retained provenance. `Layout` can be copied,
stored or discarded without allocation.

## Workers

Allocator handles and byte blocks may be moved to workers or shared through
checked borrows. Lists keep their ordinary element-dependent worker and borrowing
rules: selecting an allocator does **not** make every `list[T]` thread-shareable
or permit concurrent mutation. No allocator type parameter is added to the list.
Byte blocks follow ordinary ownership and borrowing rules, allowing exclusive
write access only through `&mut` and shared read access through `&`.

```plenty
import std.alloc as alloc

def worker(owner: alloc.Allocator, input: i64) -> Result[i64, AllocError]:
    mut values = owner.empty_list[i64]()?
    values.append(input)?
    Ok(len(values))

def main() -> Result[(), AllocError | IoError | SpawnError[()]]:
    owner = alloc.system()
    with spawn(worker, owner.share(), 99).drop_job()? as task:
        print(task.join()?)?
    Ok(())
```
```output
1
```

## Current boundaries

There is no ambient selected allocator or fallback selection in this API.
Existing allocating syntax and standard-library operations still use their
legacy paths. Copying, owning slices, sorting scratch space, and allocations
performed while producing elements are not redirected by selecting a list's
storage. This is not yet comprehensive explicit allocation control.

`ByteBlock` is allocator backing, not a replacement for the general-purpose
[`Bytes`](bytes.md) transport buffer. There is no public custom-provider
registration, typed-storage consumer, byte-view API or reset operation here.

Retained allocator use cannot coexist with actor execution, abandonment or heap
operations anywhere in the same lowered program, including unrelated lowered
functions. Importing allocator and actor modules together can therefore be
rejected even without calls from `main`, including programs that only call the
pure `layout` factory. Private binding entry points also reject ambient actor/arena contexts
before allocating or transferring ownership. Trusted separately compiled hosts
must keep retained owners out of actor/arena invocations; the binding checks do
not establish arbitrary foreign provenance or escaping lifetimes.

See [retained allocator storage](../toolchain/02-architecture/allocator-storage.md)
for native contracts and [allocation and traps](../language/08-errors/02-allocation-and-traps.md)
for recoverable allocation errors and callback traps.

## Acknowledgements

- [Zig](https://ziglang.org/documentation/master/#Allocator) inspired explicit
  allocator passing. No Zig code is included.
