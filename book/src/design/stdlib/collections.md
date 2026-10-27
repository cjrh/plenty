# std.collections

`from std.collections import Heap, HeapBy, drain, drain_by` provides owned
binary heaps (priority queues). They are ordinary embedded Plenty code over a
private `list[T]`; no native code or runtime primitive is involved.

| Type | First element | Element types |
|---|---|---|
| `Heap[T]` | Least by `<` | Integers, floats, `str`, and tuples of those |
| `HeapBy[T, F]` | The one the comparator puts before all others | Any storable type |

`Heap[T]` uses the element type's own `<`. Classes, ordinary enums, collections,
and tuples containing unordered fields are rejected at compile time with
"operator `<` does not accept" the type. A scalar NaN element compares false
against everything and leaves the removal order unspecified. Tuples use the
[lexicographic total sorting order](../language/03-types/tuples.md), including
signed zeros and NaNs, so `Heap[(i64, str)]` orders by priority then text.

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
| `heap.peek()` | `Option[T]` | Copy the first element; copyable `T` only |
| `heap.peek_ref()` | `Option[&T]` | Borrow the first element |
| `heap.pop()` | `Option[T]` | Remove and transfer the first element |
| `heap.clear()` | `()` | Destroy all elements, retaining capacity |
| `drain(heap)` | `Generator[T]` | Consume a `Heap[T]`, yielding elements in removal order |
| `drain_by(heap)` | `Generator[T]` | The same for a `HeapBy[T, F]` |

Mutation requires a mutable owner or exclusive reference. On an empty heap,
`pop`, `peek` and `peek_ref` return `Nothing`. `peek` follows the
[copying lookup rule](../facilities/01-builtins/collections.md#copying-and-borrowed-lookup):
it type-checks only for a copyable `T`, and is checked where it is called, so a
heap of class instances uses `peek_ref`. The reference from `peek_ref` borrows
the heap, so the heap cannot change while it is live. There is no `peek_mut`:
changing the first element in place could break the heap order.

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
| `len`, `is_empty`, `peek`, `peek_ref` | Constant |
| `push` | At most `floor(log2 n)` comparisons; a growing push also moves the backing list |
| `pop` | At most `2 * floor(log2 n)` comparisons |
| `clear`, drop | Linear, plus element cleanup |
| Draining all elements | `O(n log n)` comparisons |

Supported target: `x86_64-unknown-linux-gnu`. See the runnable
[priority queue lesson](../../tutorial/07-modules/05-priority-queues.md).

## Limits

There is no construction from an existing list, bulk insertion, merging,
indexed access, borrowed iteration, removal of an arbitrary element, or
priority update. A comparator cannot fail; one that reports inconsistent answers
gives an unspecified order but cannot break memory safety.

## Acknowledgements

- Modelled on Python's [`heapq`](https://docs.python.org/3/library/heapq.html), which also puts the least element first, and Rust's [`BinaryHeap`](https://doc.rust-lang.org/std/collections/struct.BinaryHeap.html).
