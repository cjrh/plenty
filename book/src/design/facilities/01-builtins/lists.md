# Lists

Lists use the [common collection rules](collections.md). Growth through
`append`, `insert`, `reserve`, and `extend` is recoverable; indexed replacement allocates
nothing and traps on a missing index. Lists have no `*` repetition; a
comprehension such as `[0.0 for _ in range(count)]?` builds repeated elements.

With `--unchecked assume`, list indexing lexically inside a plain
[`unchecked:` suite](../../language/03-types/numbers.md#unchecked-blocks)
requires a nonnegative index below the list length. Reads, writes and borrows
omit both the bounds check and negative-index normalization; violating this
precondition is undefined behavior. `unchecked trap:` and default builds keep
normal indexing behavior. The optimizations below describe checked accesses.

Inside `for` and `while` loops, scalar indexing of a named list already in
scope reads the list's length and row address once, before the loop, and then
uses checked direct loads and stores. An immutable shared reference always
qualifies. An owned list, exclusive reference, or retargetable binding
qualifies only when the loop cannot resize, replace, or move the list: the loop
makes no call and performs no list operation other than scalar element reads,
writes, and length queries. Each list keeps its own bounds check, and a
negative index is still normalized; unrelated lists need not have equal
lengths. Generator bodies and other loops keep ordinary indexed access.
In nested loops, an immutable shared reference bound before the outer loop is
read once, before the outermost loop that follows its binding. Any other list
is read before the outermost enclosing loop that meets the same condition and
follows its binding, and sibling inner loops share that read (issue #943). An
enclosing loop that appends to the list, replaces or retargets the binding, or
passes it mutably to a call keeps the read inside it, before the inner loop.
A length that is the inner loop's own `range(len(xs))` stop also stays there.

An access that cannot fail has no bounds check. In
`for i in range(start, len(xs))` with a nonnegative constant `start` and
step 1, `xs[i]` cannot fail when `xs` is a `&list[T]` parameter or a
[slice view](../../language/04-expressions/slices.md#slice-views): its
length cannot change while the function runs. The stop may also be a
local set from `len(xs)` earlier and not changed since, as in
`n = len(xs)` followed by `for i in range(n)`. With the stop written as
`len(xs)` for the same list `xs`, an owned list, an exclusive reference, or a
shared reference made inside the function also qualifies when the loop uses
the row cache described above: the loop cannot change the list, so the cache
takes its length from the range's stop; a length saved in a local before
the loop does not qualify for these lists. Indexing any other list with `i`,
a range with any other stop, or a loop that can resize the list keeps the
check, so a length read before a `clear()` still traps. The counter of
`for i, x in enumerate(xs)` over such a list or view qualifies the same way,
when the call has no start; indexing another list with it still checks the
bound but skips negative-index normalization.

`for x in xs` over an owned list reads the list's length and rows once,
before the loop, like iteration over a borrowed list: the loop owns the
list, and no other code can reach it. Each element read has no bounds check,
and the position advances without an overflow check.

A `while lo < hi` loop over a slice view checks once, before the loop, when
its body begins by indexing the view with `hi`, and possibly `lo`, as
`v.swap(lo, hi)` or `t += v[hi]` does. The view must be one the loop does not
reassign, `lo` must start nonnegative and only grow, and `hi` must change only
by checked subtraction of constants. Each pass's `hi` is then at most its
first value, and `lo` is below `hi`. The check runs only when the loop does,
and fails with the first pass's message and site. A body that starts with any
other operation that can fail, such as `t * 10 + v[hi]`, keeps its checks.

Any other `for i in range(start, stop)` loop with step 1 and a nonnegative
`start` is compiled twice when its body is small, makes no call, and has no
nested range loop, as in `for i in range(n): dst[i] = src[i]`. `start` is
nonnegative when it is a constant, a range counter, or a checked sum of such
values, as in `for j in range(i + 1, n)`. One test before the loop compares
`stop` with the length of each list or view the body indexes by `i` or `i + k`,
for a constant `k` from 0 to 64, and checks each index that no pass changes,
such as `bodies[i]` in that inner loop. When every index is in bounds, the copy
without those checks runs; otherwise the checked copy runs, so a body that
breaks, returns, or fails before reaching a bad index behaves as before, and
the bad index traps with its usual message and site. A negative unchanged
index also selects the checked copy, which counts it from the end. The length
must be read once before the loop: a borrowed list or view, or a list that
uses the row cache described above. A division keeps its own checks in both
copies. The body limit, 256 operations, bounds the code size the second copy
adds. An overflow check on `i + k` with `k > 1` remains.

A `while` loop without the `lo < hi` form above is compiled twice under the
same conditions. Either kind of loop may hold a nested `while` loop. Its test
covers each unchanged index and each nonnegative constant index, such as
`perm[0]` in `while perm[0] != 0:`; a negative constant index keeps its check
in both copies. A nested
loop also sees the lengths its enclosing loops never change: with
`n = len(u)`, a `for j in range(n)` loop inside `for i in range(n)` reads
`u[j]` and `u[i]` without checks.

An index widened from a `u8` or `u16` value, as in the table lookup
`table[i64(*byte)]`, is never negative and is at most 255 or 65,535. It needs
no negative-index normalization, and the test before the loop compares that
largest value with the table's length. A table with more entries than that
value (at least 256 for a `u8` index, 65,536 for a `u16`) runs the copy
without the check; a shorter one runs the checked copy, which reports an
out-of-range index as before (issue #760). An `i8` or `i16` source
may be negative and keeps its checks.

## Storage costs

Scalar lists store elements at their declared widths: one byte for `bool`, `i8`
and `u8`; two for `i16` and `u16`; four for `i32`, `u32` and `f32`; eight for
`i64`, `u64` and `f64`. Thus 128 live `u8` elements occupy 128 bytes of row
storage, and 128 `i64` elements occupy 1,024 bytes. The allocation also includes
a fixed owner, spare capacity from growth, and rounding to 16-byte allocation
units. `reserve` counts elements, not bytes.

Other elements use one 16-byte value slot plus any inline payload storage;
lists reserve no dictionary value slot. Scalar-only `Option` and `Result`
elements still take 16 bytes. Records and multi-field enum variants retain
16-byte field slots, so an optional variant with three `bool` fields takes
64 bytes per row. These are private implementation layouts, not an FFI ABI.
Element references remain valid under the existing borrowing rules, including
for packed scalar elements. See [value representations](../../toolchain/02-architecture/values.md).

## Lookup and removal

| Operation | Result | Behavior |
| --- | --- | --- |
| `items.get(index: i64)` | `Option[T]` | Observe an element; `Nothing` outside the list |
| `items.get_ref(index: i64)` | `Option[&T]` | Lend an element; `Nothing` outside the list |
| `items.get_mut(index: i64)` | `Option[&mut T]` | Lend an element exclusively; `Nothing` outside the list |
| `items.pop()` | `Option[T]` | Remove and own the last element |
| `items.pop(index: i64)` | `Option[T]` | Remove and own the selected element |
| `items.count(value)` | `i64` | Number of equal elements |
| `items.find(value)` / `items.rfind(value)` | `Option[i64]` | First/last matching position |

Indices can be references; negatives count from the end. `get`, `get_ref`,
`get_mut`, and `pop` handle extreme `i64` indices safely. None allocates; a
missing `pop` leaves the list unchanged. `pop` requires exclusive access, preserves remaining order and capacity,
and supports all list element types. Last-element removal touches no surviving
rows and is constant-time in the list length, including for inline elements.
Other positions shift and repair only subsequent rows. Extracted inline payloads
are copied into the caller's own storage before another list mutation or
destruction; the freed capacity can be reused by a later `append`.

`get` observes the list and index once in source order. It supports only
non-affine values, including copyable enums; retained results survive replacement
or destruction of the list. Use `pop` or explicit references for affine elements.
There is no default argument.

`get_ref` and `get_mut` lend the element at `index` in place, for every element
type, or return `Nothing` outside the list instead of trapping as `&items[index]`
and `&mut items[index]` do. They normalize negative indices as indexing does, so
`items.get_mut(-1)` lends the last element. The index is observed before the
receiver is borrowed. The result holds a loan of the whole list until its last
use, shared for `get_ref` and exclusive for `get_mut`, so the list cannot grow,
shrink, or be reassigned meanwhile. `get_mut` requires a mutable binding, a
mutable reference, or a class field reached through one.

Search/count accept exactly the element type, including references, and take
every element type that `in` takes: numbers, `bool`, strings, classes, enums,
tuples, nested lists, and unions of them. They use the element type's `==`,
observe both operands once, allocate nothing, and use IEEE float equality (NaN
never matches; signed zeros do). Numbers, `bool`, and strings use a runtime
scan; other element types compile to a loop specialized for the element type,
the one a [slice view](../../language/04-expressions/slices.md#operations-on-views)
uses. Optional bounds and throwing `index` are unsupported; search part of a
list through a view, `(&items[a:b]).find(value)`, which counts positions within
the view.

## Insertion

`items.insert(index: i64, value: T) -> Result[(), AllocError]` places `value`
before the element at `index`, so it then occupies that position; later
elements move up one place. `index` may be any position from `0` to
`len(items)`, where `len(items)` appends. Negative indices count from the
end, so `-1` inserts before the last element and `-len(items)` at the front.
Any other index traps, reporting `list.insert: index 5 is out of range for length 2`
and the calling line, before the list or its storage changes. Python instead
clamps such an index to the nearest end; Plenty treats it as a bug in the
caller, as it does an out-of-range read, write or `swap`.

Insertion requires exclusive access and accepts every element type. The index,
which may be a reference, is evaluated first, then the value, both before the
list is borrowed, so either may read the list. Growth follows `append`: storage
is reserved before any element moves, capacity grows geometrically, and a
failure leaves the list unchanged and drops `value`. Reserve first to keep an
owned value that must survive a storage failure. With spare capacity, insertion
allocates nothing.

The later elements shift with one memory move, without copying, retaining, or
dropping them; inserting at `len(items)` moves none. Inline payload addresses of
the shifted rows are repaired afterwards, as for `pop(index)`. Insertion is
O(n - index) in time, so repeated insertion at the front is quadratic; a
[deque](../../stdlib/deque.md) adds at either end in amortized constant time.

```plenty
def main() -> Result[(), AllocError | IoError]:
    mut steps = ["mix", "bake"]?
    steps.insert(0, "measure")?
    steps.insert(-1, "pour")?
    steps.insert(len(steps), "cool")?
    print(steps)?
    Ok(())
```

```output
["measure", "mix", "pour", "bake", "cool"]
```

## Reordering and extending

`items.reverse() -> ()` requires exclusive access and reorders all element types
without allocation, copying, retaining, or dropping them. Empty and singleton
lists are unchanged; later destruction follows the new order.

`items.swap(i: i64, j: i64) -> ()` exchanges two elements under the same
contract: exclusive access, every element type, and no allocation, copy, retain,
or drop. Negative indices count from the end, and a missing index traps. Both
indices are evaluated before the list is borrowed, so they may read it. Equal
indices leave the list unchanged. `swap` is the way to exchange owned elements,
which cannot be moved out of indexed storage. On a list of numbers or `bool`,
`swap` compiles to inline bounds checks, loads, and stores with no runtime call;
other element types call the runtime once. An exclusive
[slice view](../../language/04-expressions/slices.md#slice-views), `&mut [T]`,
has the same `swap` and `reverse`, with indices counted within the view.

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
result is fully determined. `items.sort(True)` orders descending; an omitted or
`False` argument orders ascending. Sorting takes O(n log n) comparisons.
Wide inline rows use three-way introsort with insertion sort for small partitions
and a heapsort fallback; rows without inline payloads use Rust's unstable slice sort. Row swaps
repair inline addresses immediately, without retaining or dropping elements.
The wide-row algorithm uses O(log n) stack space and no heap workspace. A tuple
of numbers and strings has its field offsets resolved once per sort.

`items.sort_by_key(key) -> ()` accepts any element type. The key is a named
function, a reusable shared-call closure, or an anonymous function written in
place, such as `items.sort_by_key(def(x: &i64) -> i64: *x)`; it takes `&T` for the element type `T` and returns an
integer, float, string, or tuple of those. `items.sort_by_key(key, True)` orders
descending. The list is exclusively borrowed for the operation; callbacks
cannot also borrow it through a capture. Keys use the same total order as
`sort`, including signed zero and NaN placement. The runtime sorts the rows
with the wide-row introsort above, for every element width, and calls a
generated comparator that computes both keys. The sort is unstable: equal-key
elements may change relative order. It allocates no buffer and recomputes keys
during O(n log n) comparisons, so the callback should give consistent keys and
remains responsible for its own explicit allocations. An inconsistent key
yields an unspecified order but keeps every element.
A key returning `Result[K, E]` is also accepted: the sort then returns
`Result[(), E]` instead of `()`. The first error stops further key calls;
the runtime finishes its safe row moves and returns that error unchanged.
The list may already be reordered, but every element remains owned exactly
once. Both keys and errors may own strings. Callers handle this public result
normally, even inside `unchecked:`; no private callback channel is involved.
For example, a negating key declares
`def(x: &i64) -> Result[i64, OverflowError[i64]]: Ok(-*x)`.
One-shot and mutable-call closures and reference-valued keys are unsupported.
Elements are moved by row exchange, with no copying or early destruction.

`items.sort_stable_by_key(key) -> Result[(), AllocError]` takes the same keys
and the same optional `descending` flag as `sort_by_key`, and keeps elements with equal
keys in their original relative order, including in descending order. Records
ordered by a timestamp need no position tie breaker. `Timestamp` is not itself
a key; use the tuple `(t.seconds, t.nanoseconds)`, which orders the same way.
Workspace is the cost of stability: a single scratch block of `floor(n / 2)`
rows is reserved before any row moves. If that allocation fails the list is
unchanged and the call returns `AllocError`. Input already in key order is
detected with `n - 1` comparisons and returns without allocating or moving.
Otherwise a top-down merge sort uses insertion sort for runs of at most 16
rows, skips merging two runs already in order, and takes O(n log n)
comparisons, each computing two keys, plus O(log n) stack. The scratch block
is freed before the call returns. Moves into and out of scratch repair inline
addresses, and every element stays owned exactly once, even when a key is
inconsistent; an inconsistent key yields an unspecified order.
A key returning `Result[K, E]` makes the result `Result[(), AllocError | E]`,
or `Result[(), AllocError]` when `E` is `AllocError`. As with `sort_by_key`,
the first key error stops further key calls, the runtime finishes its safe
moves, and the call returns that error; the list may be partly reordered.
On allocation failure the list is unchanged, but the ordered-input check may
already have called the key. The two errors never occur together: the
allocation is attempted only after that check finds an out-of-order pair, and
after a key error every comparison reports equal, so the check cannot find one.

An exclusive [slice view](../../language/04-expressions/slices.md#operations-on-views),
`&mut [T]`, has `sort`, `sort_by_key` and `sort_stable_by_key` with these
contracts, reordering only the view's elements. Its owner stays exclusively
borrowed, so a key cannot capture the owner either. `sorted` takes a list, not a view;
copy a view with `list(v)?` and sort the copy.

`sorted(items) -> Result[list[T], AllocError]` observes a list and explicitly
copies it before sorting by natural order. `sorted(items, True)` gives a
descending copy. The source survives either result; the element type must
support copying and natural ordering. For key ordering of a copy, use
`mut result = copy(items)?` followed by `result.sort_by_key(key)` or
`result.sort_stable_by_key(key)?`.
User-defined comparison functions and class ordering protocols are unsupported.

`items.extend(other) -> Result[(), AllocError]` consumes a same-typed owned list
on both outcomes. It reserves the entire additional count before transfer.
Success moves elements in order without cloning; failure preserves destination
contents and drops the source. Sufficient capacity, including empty sources,
needs no allocation. Source references, arbitrary iterables, and self-extension
by moving the destination are rejected. Use `extend(copy(source)?)` to preserve it.

## Slices

[Slice syntax](../../language/04-expressions/slices.md) `items[start:stop:step]?`
copies the selected elements into a new list with Python's clamping, omitted
bounds, and negative steps. It duplicates elements with `copy()` and `Shared`
handles with `share()`, and leaves the source unchanged; element types that
`copy()` rejects are a compile error. A [slice view](../../language/04-expressions/slices.md#operations-on-views)
copies the same way with `list(v)?` or `v[a:b]?`.

`items.slice(start: i64, stop: i64) -> Result[list[T], AllocError]` is the
transfer form for an owned temporary. It takes both
bounds, observes receiver and bounds once in order, and accepts references.
Start is inclusive; stop is exclusive. Negative bounds count from the end and
clamp to `[0, len(items)]`; reversed or extreme bounds safely yield the corresponding
clamped slice. It has no step.

Non-affine elements are retained into an independent list. Affine elements
require an owned temporary receiver: selected owners transfer, and the remainder
drops in source order. Failure drops the whole temporary; observed sources remain
unchanged. `copy(items)?.slice(start, stop)` explicitly preserves an affine source.
All output storage is reserved before transfer: two allocations for a nonempty
slice, one for an empty owner.

## Sorting design alternatives

[User-defined ordering](../../../proposals/user-type-ordering.md) is an
unimplemented proposal for comparison semantics. The
[Timsort proposal](../../../proposals/timsort.md) records the stable-sort
alternatives; `sort_stable_by_key` implements its explicit stable key sort with
a plain merge sort, without Timsort's run detection or galloping. Natural-order
`sort` has no stable form because elements that compare equal are
indistinguishable.
