# `std.experimental.alloc` — allocator experiments

**Experimental B13 probe, not a general allocator API.** This module tests explicit
allocator selection, retained ownership and bounded arena storage with an
integer-only collection. Builtin `list`, strings, boxes and other std modules
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

## Boundary and limitations

Both policies are implemented in Plenty. Private trusted C declarations are a
prototype exception to the std-intrinsic convention: ordinary Plenty currently
cannot allocate or address allocator-selected raw storage. `malloc`/`free` are
the only foreign calls; scalar access and pointer offsets use the existing
trusted interface instructions. No raw storage operation is public.

The private pointer type has no thread contract, so allocators and lists cannot
cross workers. Reference counts and bump state are deliberately non-atomic.
The implementation assumes the compiler's current 64-bit pointer targets and
supports only eight-byte layouts. It does not support arbitrary allocator
implementations, generic elements/destructors, resettable arenas, shared workers,
actor containment, or integration with builtin allocation-failure accounting.
The C system backend is not the runtime's small-block pool. Tests inject failures
into its actual `malloc` calls and additionally run under Valgrind.

## Acknowledgements

- [C `malloc`/`free`](https://man7.org/linux/man-pages/man3/malloc.3.html)
  provide the prototype system backing allocator; no implementation is bundled.
- [Zig](https://ziglang.org/documentation/) is the API model for explicitly
  passing allocator values. No Zig code is included.
