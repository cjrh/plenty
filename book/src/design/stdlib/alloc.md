# `std.alloc` — explicit retained allocators and safe byte blocks

`std.alloc` exposes an explicit system allocator and compiled custom providers.
It constructs ordinary `Box[T]`, `list[T]`, `dict[K, V]` and `set[T]` owners.
Each retains the selected allocator for its own storage, so growth and cleanup
do not require the caller to keep the allocator handle. This is not comprehensive allocation control; see the
[remaining B13 scope](../../backlog.md#b13-explicit-allocator-implementation-sequence).

This module also provides validated memory-layout requests and affine
zero-initialized byte blocks with checked access and disjoint splitting. These
are safe allocator backing types for custom policies: no raw pointers escape,
no copying is permitted, and blocks retain their provider through all operations.
Fresh block construction allocates metadata even for an empty range. Valid
layouts do not guarantee that metadata and payload can both be allocated.

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

### Allocator handles and lists

| Signature | Ownership, allocation and errors |
| --- | --- |
| `system() -> Allocator` | Creates an explicit system-root handle without allocation. It has no per-instance state or reference count to allocate. |
| `Allocator.share(self) -> Allocator` | Borrows the handle and returns another owning handle without allocation. Custom handles atomically retain their state; the system root has no reference count to increment. |
| `Allocator.empty_list[T](self) -> Result[list[T], AllocError]` | Borrows the allocator and creates an empty builtin list with zero row capacity. Object and bookkeeping storage still allocate. |
| `Allocator.list_with_capacity[T](self, capacity: i64) -> Result[list[T], AllocError]` | Borrows the allocator and creates an empty builtin list with at least the requested element capacity. Zero is valid; negative or unrepresentable capacity returns `CapacityOverflow`. |

### Dictionaries and sets

| Signature | Result on success |
| --- | --- |
| `Allocator.empty_dict[K: KeyType, V](self) -> Result[dict[K, V], AllocError]` | An empty dictionary; no entries are reserved. |
| `Allocator.dict_with_capacity[K: KeyType, V](self, capacity: i64) -> Result[dict[K, V], AllocError]` | An empty dictionary with room for at least `capacity` entries. |
| `Allocator.empty_set[T: KeyType](self) -> Result[set[T], AllocError]` | An empty set; no hash entries are reserved. |
| `Allocator.set_with_capacity[T: KeyType](self, capacity: i64) -> Result[set[T], AllocError]` | An empty hash-backed set with room for at least `capacity` members, or a bitset with room for its whole domain. |

All four methods borrow the allocator. Even an empty owner allocates. For a
hash-backed collection, the object, bookkeeping, rows, insertion-order links
and buckets all use the selected allocator, including later reserve and rehash.
Partial construction releases any storage obtained. If growth fails, existing
entries and their lookup behavior are unchanged. Provider errors propagate as
`AllocError`, without retrying through the system allocator.

Both capacity arguments count entries, not bytes. Zero is valid; a negative or
unrepresentable request returns `AllocError.CapacityOverflow`. For a
[finite-domain bitset](../facilities/01-builtins/sets.md#small-enum-bitsets),
the constructor still checks the ordinary entry-count limit before calling the
provider. A valid request larger than the domain is allowed: the owner allocates
once, with room for every possible member regardless of the hint. Valid reserve
and insertion need no further backing allocation.

The [ordinary collection rules](../facilities/01-builtins/collections.md) apply:
keys and members must satisfy `KeyType`; dictionary values must be heap-storable.
For example, a reference or generator cannot be a stored value, and hidden task
or channel ownership is rejected. Selecting a table does not select storage for
nested values, even a string key constructed from bytes. Neither `copy` nor
ordinary set methods inherit an input's allocator for their result.

### Set algebra with an output allocator

| Signature | Members of the result |
| --- | --- |
| `Allocator.union[K: KeyType](self, left: &set[K], right: &set[K]) -> Result[set[K], AllocError]` | In either set |
| `Allocator.intersection[K: KeyType](self, left: &set[K], right: &set[K]) -> Result[set[K], AllocError]` | In both sets |
| `Allocator.difference[K: KeyType](self, left: &set[K], right: &set[K]) -> Result[set[K], AllocError]` | In `left` but not `right` |
| `Allocator.symmetric_difference[K: KeyType](self, left: &set[K], right: &set[K]) -> Result[set[K], AllocError]` | In exactly one set |

For `out.union(&a, &b)`, `out` chooses the provider. The two borrowed sets may
use different providers; neither is chosen implicitly. Both must have the same
key type. Unlike the [ordinary set methods](../facilities/01-builtins/sets.md#set-algebra),
these methods do not accept dictionary key views.

`K: KeyType` alone does not guarantee that a key can be duplicated. A legal
affine record key that supports explicit copying works without `@copy`. A key
with custom cleanup (`__del__`), even in a nested field or variant payload, is
rejected at compile time, even if the input sets are empty.

The result is an independent `set[K]` that retains `out`'s provider for growth
and cleanup. Result storage, any scratch storage and any new allocations needed
to copy keys use that provider. Already shared leaves such as immutable strings
retain their existing backing. A finite-domain bitset needs just the result
owner allocation. If allocation fails, the operation releases partial output
and returns the `AllocError` without fallback. Both inputs remain unchanged on
success and failure.

### Copy with an output allocator

`Allocator.copy[T](self, value: &T) -> Result[T, AllocError]` borrows both
inputs. Use `out.copy(&value)?` to duplicate an existing value without moving
it. When the copy needs a new `Box`, list, dictionary or set, even inside a
record or active enum payload, `out` pays for the owner and its backing. These
owners keep that provider for growth and destruction after `out` is dropped.
Unlike `out.box(value)`, this does not move nested owners with their original
storage. Unlike builtin [`copy(value)`](../facilities/01-builtins/construction-copy.md#explicit-copying),
it chooses the destination explicitly.

Inline fields need no heap allocation of their own. Immutable strings can keep
the same backing; a nested `Shared` handle gains a reference to the same value,
not a copy of its contents. Selection does not migrate those shared leaves.
The walk is bounded by the existing value, not a producer; there is no
separate size parameter. If the provider refuses a request, the copy releases its partial
output and returns the `AllocError` without retrying through the source or
system allocator. The source stays usable on either return.

`T` must satisfy the builtin explicit-copy rules but need not implement
`CopyType`: a class with an owned list field can be copied without `@copy`,
but recursive types, generators, captured closures and records with custom
destruction cannot be duplicated. For a bare `Shared`, use `handle.share()`.

Selected copy additionally requires that `T` cannot carry a loan. For example,
a class with a `data: &[i64]` field is rejected: copying the field would copy
the view, not its backing. Wrapping a borrowed value does not remove its loan.
To copy the elements of a view, use a [slice-view materializer](../language/04-expressions/slices.md#operations-on-views).
The ordinary worker borrow and transfer rules and the module's
[actor exclusion](#current-boundaries) still apply.

### Selected boxes and collection

| Signature | Ownership, allocation and errors |
| --- | --- |
| `Allocator.box[T](self, value: T) -> Result[Box[T], BoxError[T]]` | Borrows the allocator and moves `value` into one ordinary `Box[T]` allocation whose storage uses the selected allocator. Failure returns the unchanged value. |
| `Allocator.list[T](self, source: Generator[T]) -> Result[list[T], AllocError]` | Borrows the allocator and consumes `source`, building a new list with the selected allocator. Growth allocates as `append` does; no capacity is reserved from the source. |

`BoxError[T]` has one variant, `Allocation(T, AllocError)`. On failure it returns
the original value unchanged; nothing is moved into the box, and no fallback
allocator is tried.

A successful `Allocator.box` transfers the value into an ordinary concrete
`Box[T]`. Moves, borrows, `*box` extraction and destruction follow the
[boxes](../facilities/01-builtins/boxes.md) rules; the result is not a protocol
object or callable adaptation. The box keeps its allocator, so taking its content
with `*box` or dropping the box releases storage through that allocator after the
caller's handle is gone. Allocations already inside `T` keep their original
allocators. Borrowed values (`&x`) and values with hidden task or channel
ownership are rejected, as ordinary element rules require.

`Allocator.list` is ordinary `for` iteration: it creates an empty selected list,
as `empty_list` does, and appends each yielded element in order. Pass a generator
expression or generator call as the sole argument so `T` is inferred:
`owner.list(a for a in b)?`. Elements are ordinary elements, not flattened: a
generator of `Result[i64, AllocError]` produces `list[Result[i64, AllocError]]`.

The call consumes `source` on every return. On success the list owns every
yielded element. On failure, including one before the first resume, the
generator's captures, the current item not yet inserted and the partial list are
dropped exactly once, and the call returns `Err(AllocError)` with no resumable
generator. Loans held by the captures end when the call returns, so a borrowed
source can be used again afterwards.

The collector has no item limit of its own. The system root is unbounded, so a
source that never ends runs until allocation fails. An explicit provider may
refuse growth with `OutOfMemory`, which bounds the collection. Source termination
and allocations made while the generator produces elements remain the caller's
concern. When the element count is known, `list_with_capacity` reserves it
before appending.

### Custom providers

`Provider` requires public methods with these signatures (substitute the concrete
provider type for `Provider` in the receivers):

```plenty fragment
pub protocol Provider:
    def allocate(self: &mut Provider, request: &Layout) -> Result[ByteBlock, AllocError]:
        pass
    def release(self: &mut Provider, storage: ByteBlock) -> ():
        pass
```

`allocator[P: Provider](bootstrap: &Allocator, provider: P) -> Result[Allocator, RegistrationError[P]]`
borrows the explicit bootstrap and takes the provider by value. Registration
makes exactly **two allocations through that bootstrap**: a selected state box
for `P`, then the retained control allocation. There is no hidden system fallback.
Pre-existing allocations nested in `P` keep their original allocators; registration
does not migrate them.

`RegistrationError[P]` preserves ownership on failure:

- `Allocation(P, AllocError)` returns the original provider unchanged if either
  allocation fails, releasing any provisional storage without destroying `P`.
- `UnsupportedContext(P)` returns it unchanged when a native actor/arena context
  forbids registration, before allocating or transferring ownership. This is a
  runtime guard, not certification of arbitrary foreign provenance.

On success, handles and outstanding owners retain the provider and its bootstrap
storage until final cleanup. Provider callbacks are serialized. The compiler
certifies state, transitive callback effects and destructors for worker use even
when the program never spawns a worker; hidden task/channel ownership is rejected.
This does not relax ordinary element or borrow restrictions.

`allocate` receives the full backing request, including private metadata and
alignment padding, not just the user's payload size. It must return a block with
at least that extent and alignment; oversized blocks are accepted. Insufficient
extent or alignment traps before typed writes. When the backing is no longer
needed, `release` receives that **original full block with every byte zeroed**,
not a zero-length block.
The provider may cache it or drop it. Typed values have already been destroyed;
zeroing also hides padding and stale moved-owner representations.

Returned `AllocError` values remain recoverable without fallback. Traps in
allocation, release, transitive cleanup or final provider destruction are
**process-fatal**. Same-thread callback recursion traps rather than deadlocking;
cyclic dependencies between providers have no deadlock-freedom guarantee.
See the [custom-provider lesson](../../tutorial/04-collections/allocation.md#register-a-custom-provider)
for a minimal forwarding policy.

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

The list constructors return `AllocError` when storage cannot be obtained.
Partial construction releases successful allocations through their selected
allocator. A failed growth leaves the existing list intact; a failed collection
drops its partial list as described in [selected boxes and collection](#selected-boxes-and-collection).
Element types must be heap-storable and must not hide task/channel ownership
behind erasure.

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
checked borrows. Builtin boxes, lists, dictionaries and sets keep their ordinary
content-dependent worker and borrow rules. For example, a dictionary with
worker-local values cannot be moved to a worker just because its allocator can.
Selection neither adds an allocator type parameter nor permits concurrent mutation.
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
Ordinary constructors keep their existing paths. Selecting an owner changes
where it obtains its backing, not where the surrounding code allocates. For
example, ordinary `copy(value)`, dictionary key-view results, owning slices,
sorting scratch space and allocations made while producing elements or boxed
content are not redirected. `out.copy(&value)` selects only the copy itself;
it does not select storage for an expression that constructs `value`.

`ByteBlock` is allocator backing, not a replacement for the general-purpose
[`Bytes`](bytes.md) transport buffer. There is no general typed-storage consumer,
byte-view API, reset arena or raw callback registration here.

Retained allocator use cannot coexist with actor execution, abandonment or heap
operations anywhere in the same lowered program, including unrelated lowered
functions. Importing allocator and actor modules together can therefore be
rejected even without calls from `main`, including programs that only call the
pure `layout` factory. Private runtime binding entry points also reject ambient
actor/arena contexts before allocating or transferring ownership. Trusted separately compiled hosts
must keep retained owners out of actor/arena invocations; the binding checks do
not establish arbitrary foreign provenance or escaping lifetimes.

See [retained allocator storage](../toolchain/02-architecture/allocator-storage.md)
for native contracts and [allocation and traps](../language/08-errors/02-allocation-and-traps.md)
for recoverable allocation errors and callback traps.

## Acknowledgements

- [Zig](https://ziglang.org/documentation/master/#Allocator) inspired explicit
  allocator passing. No Zig code is included.
