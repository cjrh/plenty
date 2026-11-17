# `std.alloc` — explicit retained allocators

`std.alloc` exposes the explicit system allocator and constructs ordinary
`list[T]` values whose object, metadata and row storage retain the selected
allocator. Growth and destruction use that allocator after the caller's handle
is dropped. Only the system root is publicly constructible in this initial
binding; see the [remaining B13 scope](../../backlog.md#b13-explicit-allocator-implementation-sequence).

Native runtime support is necessary for builtin storage and thread-safe retained
handles. The public wrappers are written in Plenty. The separate
[`std.experimental.alloc`](experimental-alloc.md) module remains thread-confined;
its handles and owner types are not interchangeable with these.

## Example

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

## API

| Signature | Ownership, allocation and errors |
| --- | --- |
| `system() -> Allocator` | Creates an explicit system-root handle without allocation. It has no per-instance state or reference count to allocate. |
| `Allocator.share(self) -> Allocator` | Borrows the handle and returns another owning handle without allocation. For the system root there is no reference count to increment. |
| `Allocator.empty_list[T](self) -> Result[list[T], AllocError]` | Borrows the allocator and creates an empty builtin list with zero row capacity. Object and bookkeeping storage still allocate. |
| `Allocator.list_with_capacity[T](self, capacity: i64) -> Result[list[T], AllocError]` | Borrows the allocator and creates an empty builtin list with at least the requested element capacity. Zero is valid; negative or unrepresentable capacity returns `CapacityOverflow`. |

Both list constructors return `OutOfMemory` when storage cannot be obtained.
Partial construction releases successful allocations through their selected
allocator. A failed growth leaves the existing list intact. Element types must
be heap-storable and must not hide task/channel ownership behind erasure.

`Allocator` is affine: move it or explicitly call `share()`, rather than copying
it. Its representation is private. Lists retain the allocator through element
cleanup and storage release. The native system root uses explicit system
allocation rather than ambient actor/arena routing; it has no backing state to
free on final handle drop.

## Workers

Allocator handles may be moved to workers or shared through checked borrows.
Lists keep their ordinary element-dependent worker and borrowing rules: selecting
an allocator does **not** make every `list[T]` thread-shareable or permit concurrent
mutation. No allocator type parameter is added to the list.

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

Retained allocator use cannot coexist with actor execution, abandonment or heap
operations anywhere in the same lowered program, including unrelated lowered
functions. Importing allocator and actor modules together can therefore be
rejected even without calls from `main`. Private binding entry points also reject ambient actor/arena contexts
before allocating or transferring ownership. Trusted separately compiled hosts
must keep retained owners out of actor/arena invocations; the binding checks do
not establish arbitrary foreign provenance or escaping lifetimes.

See [retained allocator storage](../toolchain/02-architecture/allocator-storage.md)
for native contracts and [allocation and traps](../language/08-errors/02-allocation-and-traps.md)
for recoverable allocation errors and callback traps.

## Acknowledgements

- [Zig](https://ziglang.org/documentation/master/#Allocator) inspired explicit
  allocator passing. No Zig code is included.
