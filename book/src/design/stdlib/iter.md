# std.iter

`from std.iter import map, filter, take, fold, try_fold` provides reusable
adapters and reductions over [generators](../language/09-iteration/03-generators.md).
They are ordinary embedded Plenty generic functions.

| Function | Result | Contract |
|---|---|---|
| `map(source, f)` | `Generator[U]` | Yield `f(item)` for each item; `F: Callable[[T], U]` |
| `filter(source, keep)` | `Generator[T]` | Yield items for which `keep(&item)` is true; `F: Callable[[&T], bool]` |
| `take(source, count)` | `Generator[T]` | Yield at most `count: i64` items; zero or less yields none |
| `fold(source, initial, f)` | `A` | Combine every item into an accumulator; `F: Callable[[A, T], A]` |
| `try_fold(source, initial, f)` | `Result[A, E]` | The same with `F: Callable[[A, T], Result[A, E]]`, stopping at the first `Err` |

`source` is always an owned `Generator[T]`: a generator function call, a
generator expression, or another adapter. Lists, ranges, and other iterables
are not accepted directly; wrap one as `(item for item in values)`, which
moves `values` into the frame.

## Ownership

Every function consumes its source and its callback. A callback is a named
function or an owned closure, including one with `mut` captures; it is called
once per item, in order. Named functions are copyable, so passing one does not
use it up.

A closure with borrowed captures (`&name`, `&mut name`) is rejected with
"pass a borrowing closure by reference", and a generator expression over a
borrowed collection is rejected with "generator expressions require owned
inputs". No adapter result can therefore outlive something it borrows. Keep
running state in a `mut` capture or in the `fold` accumulator.

Items move through the chain: `map` gives each item to `f`, `filter` lends it
to `keep` and then yields or drops it, and `fold` gives the accumulator and the
item to `f` and keeps the result. Dropping an adapter before it finishes drops
its source, its callback, and any items not yet produced, without running the
rest of the source.

## Laziness, errors, and costs

`map`, `filter`, and `take` run no source code when called. Each `next`
advances the source only as far as needed for one result; `take` never
advances the source past its last item. `fold` and `try_fold` run immediately.

Adapters neither hide nor propagate errors. A fallible `map` callback yields
`Result` values for the consumer to handle. `try_fold` returns the first `Err`
and drops the unvisited source; the failing call already consumed the
accumulator.

Each adapter call builds one inline generator frame holding its source and
callback. Construction, advancement, and cleanup allocate nothing; only the
source's and callbacks' own operations can. Calls are statically dispatched
for each concrete source and callback: there is no boxing or shared iterator
type. Each item costs one callback call, and a whole chain is linear in the
items the source produces.

The [generator limits](../language/09-iteration/03-generators.md) apply:
frames nest at most 64 levels, and each concrete chain is a distinct type that
cannot be stored in a collection or class field. Frames hold no OS resource.

Supported target: `x86_64-unknown-linux-gnu`. See the runnable
[adapter lesson](../../tutorial/07-modules/06-iterator-adapters.md).

## Limits

There is no adapter over a borrowed collection, no iterator protocol for user
classes, and no `chain`, `skip`, `take_while`, `flat_map`, `collect`, `sum`,
`min`, or `max`. The loop sources
[`enumerate`, `zip`](../language/09-iteration/04-adapters.md), and
[`reversed`](../language/09-iteration/05-reverse-and-reduce.md) remain loop
and comprehension intrinsics; use them inside a generator expression to feed
an adapter. Collect results with a list comprehension.
