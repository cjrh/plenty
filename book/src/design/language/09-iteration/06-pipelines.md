# Local iterator pipelines

`iter(source)` starts a lazy, single-use pipeline. It needs no import. Source
expressions and callback captures are evaluated immediately, from left to right;
items are visited only when a loop, comprehension, or terminal consumes it.
The compiler fuses the adapters into the source's ordinary iteration loop.
Generators expose the same adapters and terminals directly:
`numbers(100).filter(even).map(square).collect()` is equivalent to starting
with `iter(numbers(100))`. Each adapter extends the fused plan.

```text
p = iter(&payments).filter(def(p: &Payment) -> bool: p.name == "bob")
total = p.map(def(p: &Payment) -> i64: p.cents).fold(0, add)
```

Use a plain loop when the body has several statements and a comprehension when
building a collection is clearest. Pipelines are useful for borrowed
transformations and reductions that would otherwise need nested generator
adapters or an intermediate collection.

## Sources and lifetime

Sources follow the [ordinary iteration rules](01-iterables.md): owned
collections and generators move into the pipeline; borrowed collections retain
their loan. Integer ranges use counted loops. Text, bytes and collection views
keep their ordinary item types.

A source may be a reference-returning call, such as `iter(owner.column())`.
The call runs once during construction; its owner stays borrowed until the
pipeline is consumed or dropped. An intermediate reference binding is optional.

A local may have an `Iter[T]` annotation. Its concrete source and callback
identity remain part of its type. Consuming it, adapting it, or `drop(p)` moves
it; a second use is rejected. Dropping it releases the owned source and
captures without advancing a generator or calling a callback. Early exit from
an owning consumer releases the unconsumed source and captures.

Pipelines cannot be returned, passed by value, shared, captured, or put in
stored data. They are currently rejected in generator functions, including
before a `yield`. A pipeline can be [lent](#lending-a-pipeline) to a
`&mut Iter[T]` parameter, and an eligible owned pipeline can leave its scope
after explicit `.generator()` conversion.

## Advancing one item

`next(p)` takes one item from a pipeline local in its creating scope and
returns `Option[T]`. The binding must be `mut`; `next(&mut p)` is equivalent.
The pipeline keeps its progress, so a later `next`, loop, adapter or terminal
continues after the items already taken:

```text
mut rows = iter(lines).filter(def(l: &str) -> bool: l != "")
header = next(rows)
records: list[Record] = rows.map(parse_row).try_collect()?
```

`next` expands the fused step inline: there is no frame and no dispatch. It
returns `Nothing` once the pipeline is exhausted, and every later `next`,
loop or terminal sees an empty pipeline: `take_while` stays stopped after its
first rejected item, and `zip` stays stopped after either side ends. Items
must be storable in an `Option`, so a pipeline of references is rejected;
`map` to an owned value first. Loans held by the pipeline stay live until it is
consumed or dropped, so mutating a borrowed source between two `next` calls is
a borrow error. `next` on a consumed pipeline is a use of a moved binding.

## Lending a pipeline

A function can take a pipeline as `&mut Iter[T]`, consume part of it, and
return. The lender then continues after the items the borrower took:

```text
def skip_header(rows: &mut Iter[str]) -> Result[(), CsvError]:
    match next(rows):
        case Some(header):
            check_header(&header)
        case Nothing:
            Err(CsvError.Empty)

def main() -> Result[(), CsvError | AllocError | IoError]:
    mut rows = iter(lines).filter(def(l: &str) -> bool: l != "")
    skip_header(&mut rows)?
    records: list[Record] = rows.map(parse_row).try_collect()?
    ...
```

The lender writes `&mut p` and the binding must be `mut`. Inside the borrower,
`p` supports `next(p)`, `for x in p`, terminals, and adapters that build a
sub-pipeline consumed in the borrower. A `break`, `return` or `?` inside the
borrower's loop leaves the remaining items in the pipeline. The borrower may
lend `p` on, as `f(p)` or `f(&mut p)`, including recursively. It cannot move,
return, store or capture `p`, and `&Iter[T]`, by-value `Iter[T]` parameters
and `Iter` results are rejected, as are `Iter` parameters of exported and
foreign functions.

`&mut Iter[T]` names only the item type. Like a `&mut Generator[T]`
parameter, the borrower is compiled once per lent pipeline, as a frame
specialization; recursive and repeated lends of the same pipeline share it.
A generic borrower such as `def first[T](p: &mut Iter[T]) -> Option[T]` infers
its type parameters from the lent pipelines' item types; supply the others
explicitly. A method can take a lent pipeline the same way. A borrower that
no call lends a pipeline to is not checked, like an unused generic function.

The lender keeps the pipeline's loans live across the call, so mutating a
borrowed source while the pipeline is unfinished stays a borrow error. Lending
a pipeline whose binding a consumer already moved is a use of a moved binding.

Costs and limits:

- The borrower receives one pointer per owned state slot (source cursor,
  owned captures, counters, `chain` phase, `flat_map` inner source, `zip` and
  `take_while` flags) and the value of each reference the pipeline holds, as
  hidden arguments. It copies owned state into its own locals on entry and
  writes it back on every exit, including `?`, `return` and an early loop
  exit. No pipeline state is heap-allocated, and a pipeline that is never lent
  pays nothing. `next(p)` in the borrower expands one step inline.
- References the pipeline holds, such as a borrowed source or a borrowed
  capture of a closure literal, are read-only in the borrower; the lender's
  loans cover them.
- Items must be owned: a pipeline of references, such as `iter(&xs)` without a
  `map` to an owned value, cannot be lent.
- A pipeline cannot be lent when it holds a reference inside other state, such
  as a closure value with borrowed captures bound before the pipeline (write
  the closure literal in the adapter call instead), or a reference into its
  own state.
- A lent pipeline may need at most 64 state slots.

## Returning a resumable stream

`.generator()` consumes a pipeline and transfers its prepared source, callback
environments, counters and other iteration state into one concrete
[generator frame](03-generators.md). It takes no arguments and adds no heap
allocation. It does not revisit source expressions, reconstruct callbacks, or
advance the source. Adapters remain fused inside the frame; `next` resumes
that frame until its next accepted item or exhaustion.

```text
def squares(stop: i64) -> Generator[i64]:
    iter(range(stop)).map(def(n: i64) -> i64: n * n).generator()

def total(values: Generator[i64]) -> i64:
    values.fold(0, def(acc: i64, n: i64) -> i64: acc + n)
```

The result can be returned, passed to a generator-consuming helper, moved,
advanced with `next`, or used as another fluent source. Owned sources and
owned captures are required. Borrowed sources and borrowed captures are
rejected even when a later `map` produces owned items; conversion never copies
borrowed state. Items must satisfy the ordinary generator yield restrictions.
The existing concrete-producer and finite-frame rules apply, including the
restriction against joining unrelated generator producer types.

Dropping an unstarted or partly consumed frame releases all its owned state
without invoking more callbacks. Exhaustion is stable. A zip frame preserves
left-first pulls and drops an unmatched left item when the right input ends.
Mutable owned captures keep their state between resumptions.

Conversion is always explicit. An expected `Generator[T]` type does not convert
an `Iter[T]`, and a generator expression over an unconverted pipeline is not
an alternative conversion spelling. Prefer local consumption when a resumable
stream does not need to cross a function boundary.

## Adapters

| Method | Behavior |
| --- | --- |
| `map(f)` | Call `(Item) -> U` once per item; emit the returned value |
| `filter(f)` | Retain items for which the shared-reference predicate is true |
| `take(n)` | Emit at most `n` items; zero or negative `n` advances nothing |
| `skip(n)` | Discard the first `n` items; zero or negative `n` skips nothing |
| `take_while(f)` | Stop at the first false shared-reference predicate, dropping that item; stay stopped |
| `filter_value(value=True)` | Retain items equal to `value`, compared with `==`; no callback |
| `enumerate(start=0)` | Emit the checked `i64` index and item; count accepted items at this point in the chain |
| `zip(right)` | Pull one accepted item from each input, left first; stop when either ends and stay stopped |
| `chain(other)` | Emit every item of this pipeline, then every item of `other` |
| `flat_map(f)` | Call `(Item) -> source` once per item; emit each item of the returned source |

Predicates receive `&T` for owned `T`, borrowed `&T`, and mutable `&mut T`
items. Mapping receives the item itself. Callback parameters and results require
explicit types. Named functions and reusable closures are accepted; captures
may be owned or borrowed. One-shot closures and indirect function values are
rejected. `map` cannot currently return references.

A closure literal written directly as a callback is lowered into the consumer's
loop instead of being called. It behaves as the call would: captures are
evaluated once, when the chain is built; borrowed captures stay borrowed until
the pipeline is consumed or dropped; owned captures are dropped with the
pipeline; and a `mut` capture keeps its state across items. A closure stored in
a local, and a literal whose body is more than one expression or contains `?`,
`match`, or another closure literal, is called as before.

`filter_value(value)` evaluates `value` once, at construction, and compares
each item with `item == value` in the loop; the item passes through unchanged.
The value takes the item type `T` as its expected type, so `filter_value(2)`
over `u8` items needs no suffix, but it is not otherwise converted; a different
type is rejected at the call. For a borrowed `&T` or `&mut T` item, `value` may
be a `T` or a shared `&T`, and the comparison reads through both references
(`*item == *value`). A borrowed value keeps its loan until the pipeline is
consumed or dropped, like a borrowed source; moving or assigning its owner
while the pipeline is live is a borrow error. Owned items require a `T`.

Any type that `==` accepts is allowed, with the same semantics. In particular,
floats use IEEE equality: a `NaN` value keeps nothing and `-0.0` keeps `0.0`.
The argument defaults to `True`, so `filter_value()` keeps true `bool` items and
is a type error for any other item type. Pair items are rejected; use `filter`
with a two-parameter predicate. An owned value is dropped with the pipeline, and
a `.generator()` conversion moves it into the frame.

Free `enumerate(source[, start])` and `zip(left, right)` also start pipelines.
`dictionary.items()` can be used directly in a chain or through `iter`.
A pair item spreads into two callback parameters: `map` and `for_each` receive
the components themselves; predicates receive shared references to both.
`fold` and `try_fold` take `(accumulator, left, right)`, and `reduce` takes
`(accumulator_pair, left, right)`. The accumulator is always one value, even
when it is a tuple. Only the item's top level spreads. Owned nested pairs are
ordinary tuples; a nested pair containing references cannot be formed.

Zip pulls through each arm's filters and skips independently. If the right
arm ends, a left item may already have been consumed; that unmatched item is
dropped. Ending a consumer drops both arms and their pending items. Enumeration
permits a final index of `i64.MAX`, trapping only if another item is available.
There is a 64-adapter/type-depth limit, alongside expression nesting limits.

`chain(other)` accepts any pipeline source, including another pipeline. Both
sides must yield exactly the same item type, pair shape included. The second
source is evaluated at construction but not advanced until the first ends. A
`take_while` that stops the first side moves on to the second; it does not end
the chain. An item borrowed from either side keeps both sources borrowed.

`flat_map(f)` calls `f` for an outer item only when the previous inner source
has ended. One rule applies: the callback returns any owned iterable (a list,
set, dictionary, `str`, `bytes`, range, or generator), and `flat_map` emits its
items as a `for` loop over that value would. A reference, a `Result`, a
non-iterable value, or a nested `Iter` is rejected; borrowed outer items are
allowed, but the returned source may not borrow them. Each inner source is released as
soon as it ends, and an unfinished one is released when the consumer exits or
the pipeline is dropped. Inner items are owned, so `flat_map` adds no loan.

## Terminals

| Method | Result and behavior |
| --- | --- |
| `fold(initial, f)` | Left fold with callback `(A, Item) -> A` |
| `reduce(f)` | Left fold without an initial value; `Option[Item]`, empty is `Nothing` |
| `count()` | Checked `i64` item count |
| `sum()` | Numeric total; zero when empty; integer overflow traps like `+` |
| `min()` / `max()` | Least / greatest numeric or `str` item in `Option[Item]`; empty is `Nothing`; ties keep the first |
| `any(f)` / `all(f)` | Short-circuit shared-reference predicates; empty is false / true |
| `find(f)` | First matching item in `Option[Item]`, then release the rest |
| `for_each(f)` | Call `(Item) -> ()` for each item |
| `try_fold(initial, f)` | Left fold with `(A, Item) -> Result[A, E]`, stopping at the callback's first error |
| `try_for_each(f)` | Call `(Item) -> Result[(), E]`, stopping at the callback's first error |
| `collect()` | Build an expected `list[T]`, `set[T]`, or `dict[K, V]`; return `Result[Collection, AllocError]` |
| `try_collect()` | Collect `Result[T, E]` items, returning `Result[Collection, E | AllocError]` |
| `generator()` | Transfer an eligible owned plan into a lazy `Generator[Item]` |

`find` and `reduce` need an item that can be stored in an `Option`; map a
borrowed item to an owned value first.

`sum`, `min`, and `max` read a borrowed `&T` or `&mut T` item as its value
(`*item`), so `iter(&values).max()` returns `Option[i64]` for a `list[i64]` and
copies the selected `str`. They take no arguments and add no allocation. `sum`
accepts numeric items; `min` and `max` accept numeric and `str` items. Pairs,
tuples, and other types are rejected: map them to a supported item first, or use
`fold` or `reduce`. Float `min` and `max` follow IEEE 754-2019 `minimum` and
`maximum`: any NaN item makes the result `Some(NaN)`, and `-0.0` orders below
`0.0`. They still consume every item. A float `sum` adds in source order.

Collecting needs an expected collection type, from a binding annotation,
argument, or return type. There is no default collection:

```text
values: list[i64] = iter(range(10)).map(square).collect()?
result: Result[list[i64], AllocError] = iter(range(10)).map(square).collect()
```

Borrowed items must be mapped to owned values before collection. Dictionary
collection consumes owned `(K, V)` pairs; the last value for a duplicate key
wins. `try_collect` into a dictionary needs `Result[(K, V), E]` items.
A failed collection drops its partial
output and the remaining owned source. `try_collect` stops at the first item
error. `try_fold` and `try_for_each` instead stop on callback errors: their
callbacks must explicitly handle any `Result` items. They add no allocation
error of their own.

## Cost

The pipeline itself adds no heap allocation or per-adapter generator frame.
Source expressions, callbacks, and destructors retain their own costs. A
generator source still pays its normal resume cost. Collecting reserves once
for a known-length range or list pipeline containing only `map`, `take`,
`skip`, `enumerate`, or `chain` of two known-length sides; a filter, `filter_value`, zip,
`flat_map`, or unknown-length source grows as it goes.
A terminal over a `chain` runs one loop per side, so it tests no flag per
item; when `zip` or `flat_map` follows the `chain`, and in `for` loops,
`collect`, comprehensions, and generator conversion, one loop tests a flag per
item. `flat_map` adds the callback and
the inner source's own setup per outer item: a list-returning callback
allocates one list per outer item, while a range-returning callback allocates
nothing.
`filter_value` adds one `==` comparison per item and no call; for scalar items
the optimized loop matches a hand-written `if *item == value` loop.
Closure literals in a chain add no call. Optimized native code also
substitutes small, straight-line scalar leaf functions and stored closures at
their call sites (at most 24 meaningful operations). Immutable scalar captures
can stay in registers. Arithmetic checks and source locations are preserved.
Larger named callbacks and stored closures retain direct calls; fusion alone
does not guarantee inline-loop performance for every body.
The reproducible [benchmark](../../../development/profiling.md#serial-pipeline-callbacks)
covers named callbacks, closure literals, scalar captures and a branching
callback.

Explicit conversion adds one suspension boundary for the whole pipeline,
rather than one frame per adapter. A generator source inside that frame still
has its own suspension boundary. Local fluent consumption keeps its fused
loop without relying on generator-frame fusion as an optimization.

The [proposal](../../../proposals/fluent-iterators.md) describes the broader
design; [B75](../../../backlog.md) tracks the remaining stages. Pipelines
replace the removed `std.iter` generator functions.
