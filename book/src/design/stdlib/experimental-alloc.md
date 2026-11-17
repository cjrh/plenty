# `std.experimental.alloc` — allocator experiments

**Experimental B13 probe, not a general allocator API.** This module tests explicit
allocator selection, retained ownership, bounded arena storage and safe external
byte-storage policies. Its collection remains integer-only. Builtin `list`, strings, boxes and other std modules
still use their existing allocation paths. See the
[allocator design and evidence](../../proposals/allocator-experiments.md).

## Small example

```plenty
import std.experimental.alloc as memory

def main() -> Result[(), AllocError | IoError]:
    system = memory.system()?
    arena = memory.arena(&system, 96)?
    mut values = arena.list(a for a in range(5))?
    print(values.len(), arena.used_bytes(), arena.live_blocks())?
    drop(arena)
    # The list retains the arena, which retains its system backing allocator.
    values.append(9)?
    print(values.get(5).unwrap())?
    drop(values)
    print(system.live_blocks())?
    Ok(())
```
```output
5 96 1
9
0
```

## Allocators

| API | Behavior |
| --- | --- |
| `system() -> Result[Allocator, AllocError]` | New independent handle/counters backed by C `malloc`/`free`. Allocates a 64-byte control block. |
| `arena(owner: &Allocator, max_bytes: NonNegative[i64]) -> Result[Allocator, AllocError]` | Reserves a 64-byte control block and up to `max_bytes` of backing storage from `owner`; retains it. |
| `Allocator.share() -> Allocator` | Another owning handle to the same allocator; no allocation. |
| `Allocator.empty_list() -> IntList` | Empty inline list owner retaining this allocator; no buffer allocation. |
| `Allocator.list(source: Generator[i64]) -> Result[IntList, AllocError]` | Consumes a generator, appending its items into a new list using this allocator. |
| `Allocator.live_blocks() -> i64` | Blocks handed out and not yet released; excludes its own metadata/backing storage, which belong to its parent. |
| `Allocator.allocation_count() -> u64` | Successful block requests since construction, including replacement buffers. |
| `Allocator.used_bytes() -> i64` | Arena bump position, including bytes from released buffers; zero for a system handle. |

All handles are affine; use `share`, not `copy`. An allocator remains alive until
its final handle, list or child arena releases it. Arenas can have arena parents.
Parent cleanup walks iteratively, not recursively. No allocator switch or
thread-local current allocator is used.

Arena storage serves eight-byte-aligned scalar slots, so the budget is rounded
down to a multiple of eight; a budget below eight cannot allocate a list buffer.
There is **no reset or individual space reclamation**. Releasing a block reduces
`live_blocks`, but not `used_bytes`. Final allocator release frees its backing
block and metadata through the retained parent. A failed arena constructor
releases any successfully acquired metadata; a bump parent still consumes those
bytes. An arena never falls back to its parent when its backing block is full.

Exhaustion is `AllocError.OutOfMemory`. Negative/impossible list capacities and
internal allocation-counter overflow are `AllocError.CapacityOverflow`. Reference
count overflow deliberately traps. A system handle's bootstrap metadata uses
`malloc` directly and is not counted in its own statistics.

## Integer lists

`IntList` owns only `i64` values. It is not builtin `list[i64]` and supplies no
indexing operator, iterator, borrowed view or generic payload layout.

| API | Behavior |
| --- | --- |
| `len() -> i64`, `is_empty() -> bool`, `capacity() -> i64` | Allocation-free queries. |
| `get(index: i64) -> Option[i64]` | Copies a scalar; returns `Nothing` for negative or out-of-range indices. |
| `reserve(capacity: i64) -> Result[(), AllocError]` | Reserves **total** capacity, without shrinking. Negative/impossible capacities fail even if no growth is needed. |
| `append(value: i64) -> Result[(), AllocError]` | Grows from zero to four slots, then doubles. Existing capacity needs no allocation. |
| `copy_into(destination: &Allocator) -> Result[IntList, AllocError]` | Copies elements into a new owner/buffer using the explicit destination. Source is unchanged. |

Growth allocates the replacement, copies initialized elements, then releases the
old buffer. Failure leaves length, capacity and contents unchanged. Destruction
releases the buffer before its retained allocator; cleanup allocates nothing.
`list` failure destroys the partial output and consumed generator. Already
performed generator effects are not rolled back. `copy_into` failure preserves
the source and cleans up the partial destination. Empty collection and copying
an empty list allocate no buffer.

The container experiment, like builtin growing containers, has no separate
input cap on `list`: an arena supplies its storage bound; a system-backed list
is limited by representable capacity and allocation failure. This is an explicit
exception to the bounded-input std convention, not guidance for parser APIs.

## Safe backing blocks

`Allocator.block(size: i64) -> Result[ByteBlock, AllocError]` returns an affine,
zero-initialized owner of exactly `size` bytes. Negative sizes and sizes greater
than `i64.MAX - 7` return `CapacityOverflow`; backing exhaustion returns
`OutOfMemory`. A private retained arena supplies the backing lifetime: creation
requests 64 bytes of metadata and `size` rounded **up** to eight bytes from the
selected allocator. Zero size still requests metadata. Failed construction
releases partial allocations, but cannot reclaim bump-parent space.

| `ByteBlock` API | Behavior |
| --- | --- |
| `len() -> i64`, `is_empty() -> bool` | Byte extent; no allocation. |
| `alignment() -> i64` | Guaranteed start alignment: 8, 4, 2 or 1 bytes, depending on split offset. An unsplit block guarantees eight. Empty ranges report the same offset-based guarantee without permitting access. |
| `get(index: i64) -> u8` | Copies one initialized byte. Invalid positions trap, naming `ByteBlock.get` and `index`. |
| `set(index: i64, value: u8) -> ()` | Requires `&mut ByteBlock`; writes one byte. Invalid positions trap, naming `ByteBlock.set` and `index`. |
| `split_off(at: i64) -> ByteBlock` | Requires `&mut ByteBlock`; keeps `[0, at)` and returns `[at, len())`, without copying bytes or allocating. Invalid positions trap, naming `ByteBlock.split_off` and `at`. |

Splits are disjoint owners, not overlapping borrowed views. Neither `copy` nor
public construction/field access can duplicate or forge an extent. Every split,
even an empty one, retains the backing. Dropping the caller's allocator and all
but one split cannot invalidate the survivor. Final release frees the backing
and metadata through their retained provider. Individual dropped ranges do not
reclaim space; no reset, merging or raw-address access is exposed. Reference-count
overflow traps as for `Allocator.share`.

```plenty
import std.experimental.alloc as memory

def main() -> Result[(), AllocError | IoError]:
    system = memory.system()?
    mut left = system.block(8)?
    mut right = left.split_off(4)
    left.set(0, 12u8)
    right.set(0, 34u8)
    drop(system)
    print(left.len(), right.len(), left.get(0), right.get(0), right.alignment())?
    Ok(())
```
```output
4 4 12 34 4
```

### External allocation policies

`BlockProvider` is a statically dispatched protocol with
`allocate(self: &mut BlockProvider, size: i64) -> Result[ByteBlock, AllocError]`.
A provider should return exactly the requested extent, reject negative/impossible
sizes with `CapacityOverflow`, and report budget exhaustion as `OutOfMemory`.
Its methods can split genuine blocks, but cannot manufacture a pointer or lie
about a block's stored extent. Provider conformance alone does not prove it
returned the requested size. The byte methods enforce bounds even for a
misbehaving provider; any future trusted typed consumer must validate extent
and alignment before raw access. This protocol
makes no alignment promise beyond the returned block's `alignment()`.

`examples/allocators/block_provider.plenty` implements an eight-byte-aligned bump
provider in an ordinary module, without trusted declarations. It rounds its
explicit budget down to eight and each request up to eight, discarding request
padding. Its `remaining_bytes` excludes allocated and discarded bytes. Failed
requests leave it unchanged; dropping an allocation does not restore the budget.
The generic consumer in `examples/allocators/owned_blocks.plenty` demonstrates
independent providers, exhaustion, disjoint mutation and allocations surviving
both provider objects and the caller's system handle. Suballocation has no
additional allocation cost.

This is a safe byte-storage policy probe, **not** a generic typed allocator ABI.
Blocks retain backing storage, not arbitrary provider objects or custom release
callbacks. They cannot yet contain typed values, invoke element destructors,
serve `IntList` storage, or promise alignment above eight. See
[B13's remaining sequence](../../backlog.md#b13-explicit-allocator-implementation-sequence).

## Boundary and limitations

Both policies are implemented in Plenty. Private trusted C declarations are a
prototype exception to the std-intrinsic convention: ordinary Plenty currently
cannot allocate or address allocator-selected raw storage. `malloc`/`free` are
the only foreign calls; scalar access and pointer offsets use the existing
trusted interface instructions. No raw storage operation is public.

The private pointer type has no thread contract, so allocators and lists cannot
cross workers. Reference counts and bump state are deliberately non-atomic.
The implementation assumes the compiler's current 64-bit pointer targets and
supports byte storage with up to eight-byte alignment and the integer-only list.
External safe block policies are supported, but arbitrary raw allocator
implementations, generic elements/destructors, resettable arenas, shared workers,
actor containment, and integration with builtin allocation-failure accounting
remain unsupported. The C system backend is not the runtime's small-block pool. Tests inject failures
into its actual `malloc` calls and additionally run under Valgrind.

## Acknowledgements

- [C `malloc`/`free`](https://man7.org/linux/man-pages/man3/malloc.3.html)
  provide the prototype system backing allocator; no implementation is bundled.
- [Zig](https://ziglang.org/documentation/) is the API model for explicitly
  passing allocator values. No Zig code is included.
