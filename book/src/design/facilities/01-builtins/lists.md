# Lists

Lists use the [common collection rules](collections.md). Growth through
`append`, `reserve`, and `extend` is recoverable; indexed replacement allocates
nothing and traps on a missing index. Lists have no `*` repetition; a
comprehension such as `[0.0 for _ in range(count)]?` builds repeated elements.

## Lookup and removal

| Operation | Result | Behavior |
| --- | --- | --- |
| `items.get(index: i64)` | `Option[T]` | Observe an element; `Nothing` outside the list |
| `items.pop()` | `Option[T]` | Remove and own the last element |
| `items.pop(index: i64)` | `Option[T]` | Remove and own the selected element |
| `items.count(value)` | `i64` | Number of equal elements |
| `items.find(value)` / `items.rfind(value)` | `Option[i64]` | First/last matching position |

Indices can be references; negatives count from the end. `get` and `pop` handle
extreme `i64` indices safely. Neither allocates; a missing `pop` leaves the list
unchanged. `pop` requires exclusive access, preserves remaining order and capacity,
and supports all list element types. Last-element removal touches no surviving
rows and is constant-time in the list length, including for inline elements.
Other positions shift and repair only subsequent rows. Extracted inline payloads
are copied into the caller's own storage before another list mutation or
destruction; the freed capacity can be reused by a later `append`.

`get` observes the list and index once in source order. It supports only
non-affine values, including copyable enums; retained results survive replacement
or destruction of the list. Use `pop` or explicit references for affine elements.
There is no default argument.

Search/count accept exactly the element type, including references, and support
integers, floats, booleans, and strings. They observe both operands once, allocate
nothing, and use IEEE float equality (NaN never matches; signed zeros do).
Aggregate searches, optional bounds, and throwing `index` are unsupported.

## Reordering and extending

`items.reverse() -> ()` requires exclusive access and reorders all element types
without allocation, copying, retaining, or dropping them. Empty and singleton
lists are unchanged; later destruction follows the new order.

`items.swap(i: i64, j: i64) -> ()` exchanges two elements under the same
contract: exclusive access, every element type, and no allocation, copy, retain,
or drop. Negative indices count from the end, and a missing index traps. Both
indices are evaluated before the list is borrowed, so they may read it. Equal
indices leave the list unchanged. `swap` is the way to exchange owned elements,
which cannot be moved out of indexed storage.

`items.sort() -> ()` orders the elements ascending, in place, without allocating.
It requires exclusive access and one of these element types:

| Element type | Order |
| --- | --- |
| Integers | Numeric |
| `f32`, `f64` | Numeric, with `-0.0` before `0.0` and every NaN after all numbers |
| `str` | By code point; a prefix comes before the longer string |
| Tuples of these types | By the first component, then the next where those are equal |

Every other element type, including `bool`, classes, enums, and collections, is
a compile-time error. Elements that compare equal are indistinguishable, so the
result is fully determined. There is no key function, comparison function,
descending order, or `sorted` expression; `reverse()` after `sort()` gives
descending order, and a list of `(key, value)` tuples sorts by key. Sorting takes
O(n log n) comparisons.

`items.extend(other) -> Result[(), AllocError]` consumes a same-typed owned list
on both outcomes. It reserves the entire additional count before transfer.
Success moves elements in order without cloning; failure preserves destination
contents and drops the source. Sufficient capacity, including empty sources,
needs no allocation. Source references, arbitrary iterables, and self-extension
by moving the destination are rejected. Use `extend(copy(source)?)` to preserve it.

## Slices

`items.slice(start: i64, stop: i64) -> Result[list[T], AllocError]` takes both
bounds, observes receiver and bounds once in order, and accepts references.
Start is inclusive; stop is exclusive. Negative bounds count from the end and
clamp to `[0, len(items)]`; reversed or extreme bounds safely yield the corresponding
clamped slice. Slice syntax, omitted bounds, and steps are unsupported.

Non-affine elements are retained into an independent list. Affine elements
require an owned temporary receiver: selected owners transfer, and the remainder
drops in source order. Failure drops the whole temporary; observed sources remain
unchanged. `copy(items)?.slice(start, stop)` explicitly preserves an affine source.
All output storage is reserved before transfer: two allocations for a nonempty
slice, one for an empty owner.
