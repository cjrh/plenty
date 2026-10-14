# std.collections

`from std.collections import Heap, HeapBy, drain, drain_by` provides owned
binary heaps (priority queues). They are ordinary embedded Plenty code over a
private `list[T]`; no native code or runtime primitive is involved.

| Type | First element | Element types |
|---|---|---|
| `Heap[T]` | Least by `<` | Integers, floats, and `str` |
| `HeapBy[T, F]` | The one the comparator puts before all others | Any storable type |

`Heap[T]` uses the element type's own `<`. Tuples, classes, enums, and
collections have none, so such an instance is rejected at compile time with
"operator `<` does not accept" the type. A NaN element compares false against
everything and leaves the removal order unspecified.

`HeapBy(before)` stores a comparator `F: Callable[[&T, &T], bool]`: a named
function or an owned closure. `before(a, b)` returns whether `a` must leave
ahead of `b`; it must be a consistent strict order and must return `False` for
equal elements. Reverse the test for a greatest-first queue. The comparator is
inferred from its signature, as in `HeapBy(urgent)?`. A closure with borrowed
captures cannot be stored.

## Operations

Both types share one interface. `heap` is `Heap[T]` or `HeapBy[T, F]`.

| Operation | Result | Contract |
|---|---|---|
| `Heap[T]()` | `Result[Heap[T], AllocError]` | Construct an empty heap |
| `HeapBy(before)` | `Result[HeapBy[T, F], AllocError]` | Construct an empty heap owning its comparator |
| `heap.len()` | `i64` | Observe the element count |
| `heap.is_empty()` | `bool` | Observe whether the count is zero |
| `heap.reserve(additional)` | `Result[(), AllocError]` | Make room for at least `additional: i64` more elements |
| `heap.push(value)` | `Result[(), AllocError]` | Consume a value and insert it |
| `heap.peek()` | `&T` | Borrow the first element; traps when empty |
| `heap.pop()` | `Option[T]` | Remove and transfer the first element |
| `heap.clear()` | `()` | Destroy all elements, retaining capacity |
| `drain(heap)` | `Generator[T]` | Consume a `Heap[T]`, yielding elements in removal order |
| `drain_by(heap)` | `Generator[T]` | The same for a `HeapBy[T, F]` |

Mutation requires a mutable owner or exclusive reference. An empty `pop`
returns `Nothing`. `peek` on an empty heap traps with an index-out-of-bounds
report, as indexing an empty list does; test `is_empty()` first. Its reference
borrows the heap, so the heap cannot change while the reference is live.

Elements that compare equal leave in an unspecified order. For first-in,
first-out ties, store an increasing sequence number in the element and compare
it after the priority.

## Ownership and cleanup

A heap owns its elements and moves like any other owner; duplicate it only
with an explicit `copy`. `push` takes ownership, and `pop` transfers it back.
Reordering exchanges rows in place: no element is copied, retained, or dropped
while it is in the heap. References, generators, and unit cannot be elements.

Dropping or clearing a heap destroys the remaining elements once each, in
storage order, not priority order. A `drain` frame owns the heap: abandoning
the frame early destroys the elements not yet yielded.

A heap holds no OS resource and no shared state. It has no thread-safety
contract of its own; it is an ordinary owned value of its element type.

## Errors and costs

`AllocError` is the only error. `push` allocates only when the backing list must grow and `reserve`
only when the requested room is missing. On failure both leave the existing
contents and order unchanged, and `push` drops the incoming value. Every other
operation, including `drain`, allocates nothing.

| Operation | Time for `n` elements |
|---|---|
| `len`, `is_empty`, `peek` | Constant |
| `push` | At most `floor(log2 n)` comparisons; a growing push also moves the backing list |
| `pop` | At most `2 * floor(log2 n)` comparisons |
| `clear`, drop | Linear, plus element cleanup |
| Draining all elements | `O(n log n)` comparisons |

Supported target: `x86_64-unknown-linux-gnu`. See the runnable
[priority queue lesson](../../tutorial/07-modules/05-priority-queues.md).

## Limits

There is no construction from an existing list, bulk insertion, merging,
indexed access, borrowed iteration, removal of an arbitrary element, or
priority update. `Heap[T]` cannot hold tuples until tuples support `<`; use
`HeapBy`. A comparator cannot fail; one that reports inconsistent answers
gives an unspecified order but cannot break memory safety.
