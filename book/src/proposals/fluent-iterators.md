# Fluent iterator pipelines

**Status: Stages 1 and 2 implemented: the serial core, pair callbacks,
dictionary collection, generator fluent methods, explicit owned `.generator()`
conversion, `chain`, `flat_map`, closure-literal splicing and comprehensions
lowered through the plan. `std.iter` was removed rather than deprecated. Stage
3's `sum`, `min` and `max` terminals, resumable adapters and `next(p)` in the
creating scope are implemented. Lending through `&mut Iter[T]` and the parallel
stage remain proposed.** Bounded native
substitution of scalar leaf callbacks also covers named functions and immutable
scalar captures; it addresses the original performance gap (issue #399).
The [pipeline reference](../design/language/09-iteration/06-pipelines.md)
lists the supported adapters, terminals, ownership rules and current limits.
Current iteration is defined by
[iteration and generators](../design/language/09-iteration/index.md),
[closures](../design/language/06-functions/03-closures.md)
and [ordered parallel mapping](../design/facilities/03-concurrency/parallel-map.md).
Related backlog entries are B12 (iterator/view protocols, borrowed adapters),
B49 (collections; its `std.iter` part was removed), B22 (broader parallel mapping) and B24 (automatic
parallelization). B75 schedules this design. Code blocks on this
page are proposed syntax unless they are labelled as current.

## Summary

Add `iter(source)`, which starts a *pipeline*: a compiler-known, lazily
evaluated chain of adapter methods (`map`, `filter`, `take`, ...) ending in a
loop, a comprehension, or a terminal method (`fold`, `count`, `collect`, ...).

```text
total = iter(&payments)
    .filter(def(p: &Payment) -> bool: p.name == "bob")
    .map(def(p: &Payment) -> i64: p.cents)
    .fold(0, def(acc: i64, cents: i64) -> i64: acc + cents)
```

A pipeline is not a generator and not a library object. The compiler lowers the
whole chain at its consumer into one loop, the same loop a hand-written `for`
or comprehension produces, with each callback called directly. Serial pipeline
machinery allocates only when a terminal builds a collection, returning `Result`.
A `.par(&pool)` step
moves the rest of the pipeline onto an explicit thread pool, reusing the
existing ordered `map` and fixed-tree `reduce` contracts.

The surface follows [excitertools](https://github.com/cjrh/excitertools): one
wrapper, chained methods, terminal "sinks". The cost model follows Rust's
`Iterator`, but fusion is done by the compiler rather than by monomorphized
adapter structs and an optimizer's inliner, which Plenty does not have.

## Starting point before B75

The original design started with iteration but no fluent interface or fused composition.
There are no builtin top-level `map` or `filter` functions; the reusable
adapters are the importable `std.iter` module.

| Facility | Form | Cost |
| --- | --- | --- |
| `for` over list, set, dict, str, bytes, range | Statement | Direct range source is a counted loop; collections use the loop plan in `op.rs` |
| `enumerate`, `zip`, `reversed` | Loop/comprehension sources only | Fused into the loop plan; no iterator object. Rejected elsewhere |
| `any`, `all` | Builtin boolean reductions | Short-circuit, no allocation |
| `dictionary.items()` | Loop intrinsic, two bindings | No item tuples |
| Comprehensions | `[e for x in s if c]` | One fused loop; output growth returns `Result[_, AllocError]` |
| Generators, generator expressions | `Generator[T]`, inline frame | No heap allocation; each advance is a runtime call (below) |
| `std.iter` | `map`, `filter`, `take`, `fold`, `try_fold` | Generic generator functions; one frame per adapter |
| `std.math.sum_iter`, `prod_iter` | Borrowed-iterable folds | Plain loop |
| `pool.map`, `pool.map_result`, `pool.reduce_tree` | Explicit parallel operations | Owned list or range input; eager, ordered |

The original `std.iter` usage was nested calls, generator sources only:

```text
# current
from std.iter import map, filter, fold
total = fold(map(filter(numbers(10), even), square), 0, add)
```

### Gaps

1. **No method chaining.** `numbers(3).map(sq)` reports "unsupported method
   `map`". Built-in types have no user-extensible methods, and protocols
   deliberately add no methods on import.
2. **Generator sources only.** A list must be moved into a generator expression,
   `(n for n in values)`. There is no adapter over a borrowed collection, since a
   frame cannot hold a reference (B12/B49 wait on B11 stored references).
3. **No borrowed captures.** Callbacks must own their captures; `def [&mut seen]`
   is rejected with "pass a borrowing closure by reference", because the frame
   that stores the callback can outlive the loan.
4. **Per-item overhead.** Every generator advance, including each adapter step,
   goes through `collection_call(24)` into the runtime dispatcher, then
   `plenty_generator_resume` (reentrancy guard), then an indirect call to the
   frame's resume function, which dispatches its state through a linear compare
   chain and keeps every local in frame memory. Nothing is inlined: the compiler
   has no function inliner, and Cranelift sees each resume function separately.
5. **No storage.** Frames cannot enter class fields, tuples or collections
   ("generators cannot be stored in generic data"), so a Rust-style adapter
   struct holding its source cannot be written in Plenty today.
6. **Missing operations.** No `chain`, `skip`, `take_while`, `flat_map`,
   `count`, `find`, `collect`, or parallel chaining.

### Measured cost

Release compiler at `6153b46f`, x86-64 Linux, summing `(n * n) % 1000` over the
even numbers below 100,000,000:

| Program | Time | vs. loop |
| --- | --- | --- |
| `for n in range(N): if n % 2 == 0: total += ...` | 0.31 s | 1x |
| Named generator with the filter inside its body, consumed by `for` | 0.62 s | 2x |
| Named generator yielding every `n`, filtered in the loop | 0.91 s | 3x |
| `fold(map(filter(numbers(N), even), sq), 0, add)` with `std.iter` | 2.74 s | 9x |
| `for v in ((n * n) % 1000 for n in range(N) if n % 2 == 0)` | 5.15 s | 17x |

The generator expression is the slowest because its outer `range` is stored in
the frame and then iterated through the runtime, like any stored range
([issue #371](#issues-found)). A stored range alone, `r = range(N)` then
`for n in r`, takes 4.71 s. Issue #371 has since been fixed: a stored range now
runs as fast as a direct range, and the generator expression drops to about
0.86 s, the cost of one frame resume per element. These numbers set the target: a pipeline should be
indistinguishable from the first row.

## Design

### Pipelines

`iter(source)` accepts every current `for` source and follows the same
[ownership rules](../design/language/09-iteration/01-iterables.md):

| Source | Item |
| --- | --- |
| `iter(values)` (owned list, set, dict) | Owned element / key; consumes the collection |
| `iter(&values)` | `&T`; borrows for the pipeline's lifetime |
| `iter(&mut values)` | `&mut T` (lists and dict values only, as today) |
| `iter(range(...))` | Integer of the range's type, counted loop |
| `iter(text)`, `iter(&buffer)` | `str` scalar, `u8` |
| `iter(generator)` | Yielded value; consumes the frame |
| `dictionary.items()` | Key/value reference pair |

`enumerate(...)`, `zip(...)` and `reversed(...)` become pipeline starts with
the same meaning as `iter(...).enumerate()` and so on. They keep their current
loop behavior, and gain chaining: `enumerate(&rows).filter(...)`. This mirrors
excitertools, whose module-level `range`, `zip` and friends also return `Iter`.

A pipeline's type is `Iter[T]` for item type `T`, but like a generator it keeps
a concrete identity (its source, adapters and closures), so two pipelines with
equal `T` do not unify. `Iter[T]` describes the item contract only.

### Where a pipeline may appear

One hard rule: **an unfinished pipeline never leaves the scope that created
it.** It may be:

- a `for` source or comprehension source;
- the receiver of a terminal method;
- bound to a local and used later; it is affine, and consuming it ends it;
- lent to a function as `&mut Iter[T]` (next section); the caller keeps
  ownership and can continue or collect it afterwards;
- turned into an owned generator with explicit `.generator()` when items must cross a function
  boundary, subject to the ownership restrictions below. That conversion is
  explicit and costs what generators cost today.

It may not be returned, passed by value, stored, captured, or held across
`yield`. Because a pipeline never outlives its scope, it can hold borrowed
sources and closures with borrowed captures. This removes gaps 2 and 3
without waiting for stored references (B11).

**Generator conversion is an explicit lowering boundary, not permission to
store an `Iter` value.** The compiler consumes the plan and transfers its owned
source, callback environments and progress into a generator frame. Conversion
is allowed only when all of that state meets the existing
[generator capture, storage and yield rules](../design/language/09-iteration/03-generators.md).
Borrowed sources, borrowed callback captures and sub-pipelines lending another
pipeline are rejected, even if their yielded items are owned. No loan may cross
`yield`, and no implicit copying makes a borrowed pipeline eligible. The
resulting generator may leave the scope; the unfinished pipeline itself may not.
Generators themselves expose the same fluent methods: `source.map(f)` builds
a pipeline equivalent to `iter(source).map(f)`, with no frame per adapter.
`.generator()` creates one frame around the complete fused plan. This spelling
replaces the originally proposed generator-expression bridge. There is no
implicit conversion from an expected `Generator[T]` type.

### Lending a pipeline to a function

A function can take a pipeline by exclusive reference, consume some of it, and
return. The caller then continues from where the callee stopped:

```text
def skip_header(lines: &mut Iter[str]) -> Result[(), CsvError]:
    match next(lines):
        case Some(header):
            check_header(&header)
        case Nothing:
            Err(CsvError.Empty)

def main() -> Result[(), CsvError | AllocError | IoError]:
    mut rows = iter(read_lines(path)?).filter(def(l: &str) -> bool: l != "")
    skip_header(&mut rows)?
    records: list[Record] = rows.map(parse_row).try_collect()?
    ...
```

Generators already work this way today: `&mut Generator[i64]` parameters
specialize to the caller's actual frame, and the caller can keep consuming
after the call (verified with the current compiler). A pipeline reuses that
model.

**What the callee may do through `&mut Iter[T]`:**

- `next(p) -> Option[T]` takes one item;
- `for x in p:` consumes items until the loop ends; after `break`, the rest
  stays in the pipeline;
- a terminal (`p.count()`, or `p.collect()` with an expected type) consumes
  everything that remains;
- an adapter (`p.take(3)`, `p.filter(...)`) makes a *sub-pipeline* that
  borrows `p` and must be consumed inside the callee. Its own closures may
  borrow the callee's locals, because it ends in the callee.

**What it may not do:** move, store, or return the pipeline, or add steps that
persist after it returns. A reference parameter cannot be stored (B11 is not
implemented), so the rule needs no new check: the pipeline stays owned by its
creating scope by construction. Loans held by the pipeline's own closures
stay live through the call, as loans held by any lent value do.

**Type.** The parameter type is `&mut Iter[T]`. Like `&mut Generator[T]`, it
names the item contract and specializes to the concrete pipeline at each call;
it is not a generic constraint. `&Iter[T]` (nothing can be advanced through a
shared reference) and by-value `Iter[T]` parameters and returns are rejected.
`Iterable[T]` was considered and rejected as the name: in Python and Rust
"iterable" means something you can start iterating, such as a list, repeatedly.
A pipeline is single-pass state, an iterator, and `Iter` matches both `iter()`
and excitertools' `Iter` class. `Iterable[T]` stays free for a possible later
constraint meaning "any source `iter()` accepts".

**Passing by value and returning it** (the alternative) would let a helper add
steps: `def evens(p: Iter[i64]) -> Iter[i64]: p.filter(even)`. It is feasible,
since generator factories already infer a concrete returned identity, but it
needs a return-derivation rule (a function may only return a pipeline derived
from a pipeline parameter, never one it created), a rule for loans in the
steps it adds, and it weakens the hard rule to "may leave if it comes back".
It is not part of this design; reusable steps are written as functions that
return closures, e.g. `.filter(longer_than(3))`.

**Complexity, stated honestly.**

- *Resumable state.* A pipeline consumed in one place keeps its progress in
  registers inside one fused loop. A lent pipeline can be consumed in several
  places, so its progress lives in slots: the source cursor, counters, `chain`'s
  phase, `flat_map`'s inner source, and `zip`'s pending left item. Each fused
  loop loads that state on entry and stores it on exit. The compiler sees every
  lend in the creating function, so pipelines that are never lent skip the
  store-back. The extra cost is per loop entry and exit, not per item; `next(p)`
  expands one step inline, still with no runtime dispatch.
- *Monomorphization.* The callee is instantiated once per distinct pipeline
  type passed to it, as with generators today. Pipeline types include closure
  identities, so two call sites with different closure literals produce two
  instances. That is code size and compile time, bounded by the existing
  specialization limits, and must be measured in `compile_bench`.
- *Diagnostics.* Errors inside an instance name the pipeline by its creation
  site, as closures are named today.
- *Every adapter must be resumable.* Each new adapter specifies its slot
  state; this is a cost on every future addition, and the reason lending is
  staged after the serial core.

### Adapters (core set)

Each adapter is lazy and consumes the pipeline it is called on.

| Adapter | Callback | Item out |
| --- | --- | --- |
| `.map(f)` | `(Item) -> U` | `U` |
| `.filter(keep)` | `(&Elem) -> bool` | `Item` |
| `.enumerate(start=0)` | none | `(i64, Item)` |
| `.zip(other)` | none; `other` is any pipeline source | `(Item, Other)` |
| `.take(n)` | none | `Item`, at most `n` |
| `.skip(n)` | none | `Item` after the first `n` |
| `.take_while(keep)` | `(&Elem) -> bool` | `Item` until the first false |
| `.filter_value(v=True)` | none; `v: Elem` or, for borrowed items, `&Elem` | `Item` equal to `v` |
| `.chain(other)` | none; same item type | `Item` from self, then other |
| `.flat_map(f)` | `(Item) -> source` | Items of each returned source |

Rules that keep callbacks predictable:

- **Predicates always receive a shared reference to the element.** For an owned
  item `T` that is `&T`; for a borrowed item `&T` it is the same `&T`, never
  `&&T`; for `&mut T` it is `&T`. `map` receives the item itself.
- **Pair items are passed as two arguments.** See
  [callbacks over pairs](#callbacks-over-pairs).
- `zip` stops at the shorter input and tests the left side first, as today.
  `chain` requires both sides to have exactly the same item type.
- `take_while` drops the first rejected item; there is no peeking.
- `flat_map`'s callback may return any owned iterable: a list, set,
  dictionary, `str`, `bytes`, range, or generator, with the items a `for` loop
  over it would yield. There are no per-type exceptions. Returning a nested `Iter` is rejected by the same no-return
  rule as any other pipeline. A callback may use an eligible explicit generator
  conversion, with the frame cost and ownership restrictions described above.

Later additions, each needing a use case: `step_by`, `map_while`, `scan`,
`inspect`, `chunks(n)` and `windows(n)` over borrowed lists (as borrowed
sub-range views, which depend on B11/B39), `dedup`, `rev` for reversible
pipelines, `zip` of more than two sources, `peekable`, `group_by`, and
`cycle`. excitertools' larger surface (`map_reduce`, `send`, I/O sources such as
`Iter.open`) stays out of the language; such sources belong in `std` modules
returning generators.

### Callbacks over pairs

Some pipelines produce two values per item:

```text
for name, score in scores.items():          # name: &str, score: &i64
for i, row in enumerate(&rows):             # i: i64, row: &Row
for a, b in zip(&left, &right):             # a: &T, b: &T
```

A loop binds the two values to two names, which works today. A callback needs
a parameter list, and the question is what it looks like. The obvious answer is
one tuple parameter, `def(pair: (&str, &i64)) -> bool`, but Plenty cannot form
that type: tuples cannot hold references ("references cannot be stored in
tuples"), because a tuple is a stored value and stored references are B11.
That is also why borrowed `zip`/`enumerate` loops today require two unpacked
bindings.

Options:

1. **Two parameters.** `def(name: &str, score: &i64) -> bool`. Works with the
   current type system; matches the loop's two bindings.
2. **One tuple parameter.** Needs tuples of references (B11), and every use
   then indexes `pair[0]`, `pair[1]`.
3. **A destructuring parameter.** `def((name, score): (&str, &i64)) -> bool`.
   Reads like the loop, but adds parameter-pattern syntax to the language and
   still names a reference tuple type.
4. **Two parameters for borrowed pairs, a tuple for owned pairs.** What the
   first draft proposed; it makes the callback shape depend on whether the
   source was borrowed.

**Recommendation: option 1, for every pair item, owned or borrowed.** The hard
rule is: *a callback over a pair item takes two parameters.* No exception for
owned pairs, and no new syntax or types:

```text
top: list[str] = scores.items()
    .filter(def(name: &str, score: &i64) -> bool: *score >= 90)
    .map(def(name: &str, score: &i64) -> str: copy(name))
    .collect()?

flagged: list[i64] = enumerate(&rows)
    .filter(def(i: &i64, row: &Row) -> bool: row.bad)
    .map(def(i: i64, row: &Row) -> i64: i)
    .collect()?
```

Only the top level is spread. In `zip(enumerate(a), b)` over owned `a`, the
inner `(i64, T)` is an owned tuple passed as one argument. An inner pair that
holds references cannot be formed and is rejected until B11; loops cannot
unpack nested pairs today either. A `map` that returns an owned tuple
produces a pair item, so the next callback takes two parameters. `collect()`
into a `dict[K, V]` target consumes owned pairs directly. If B11 later adds tuples of references, option 1
stays valid and nothing has to change.

The accumulator is separate from the item spreading rule: pair `fold` and
`try_fold` callbacks take `(accumulator, left, right)`, even when the accumulator
is a tuple. Pair `reduce` similarly takes `(accumulator_pair, left, right)`.
Predicates borrow each component; owned components therefore become `&T` and
borrowed `&T` / `&mut T` components become `&T` for the predicate call.

### Closure parameter types

Callbacks keep the current rule: parameter and result types are always
written. This proposal does not add inference.

The cost is verbosity: `def(n: i64) -> i64: n * n` where Rust writes `|n| n * n`.
The benefit is that each step of a chain states its item type where it is
read. In a long chain, the item type changes at every `map`, and without
annotations a reader must trace the chain from the source to know what `n` is
at step five. Explicit types also make errors local: a wrong callback is
reported at that callback, not as a mismatch several steps later. This
matches the project's preference for mandatory rules, and the rule is already
taught. Relaxing it later (inferring parameter types from the adapter when the
item type is known from the same chain, under B16) remains an
[open question](#open-questions); it can be added without breaking code that
writes the types.

The [closure and inference design history](typed-ranges-and-expression-inference.md#reserve-space-for-functions-as-values-and-closures)
also constrains this choice: captures remain explicit, their loans stay live
while callbacks may use them, and whether lowering inlines a callback must not
change which captures are legal. Parameter inference would not imply capture
ownership inference.

### Terminals

| Terminal | Result | Notes |
| --- | --- | --- |
| `.fold(initial, f)` | `A` | Left fold; `f: (A, Item) -> A` |
| `.reduce(f)` | `Option[Item]` | Left fold without an identity; `Nothing` when empty |
| `.count()` | `i64` | Checked; traps past `i64.MAX` like `enumerate` |
| `.any(keep)`, `.all(keep)` | `bool` | Short-circuit; predicate rule above |
| `.find(keep)` | `Option[Item]` | First match; drops the rest |
| `.for_each(f)` | `()` | `f: (Item) -> ()`; prefer a `for` loop when the body has statements |
| `.last()` | `Option[Item]` | |
| `.collect()` | `Result[C, AllocError]` | `C` is the expected `list[T]`, `set[T]` or `dict[K, V]`; see below |
| `.try_fold(initial, f)` | `Result[A, E]` | `f: (A, Item) -> Result[A, E]`; stops at the first callback `Err` |
| `.try_for_each(f)` | `Result[(), E]` | `f: (Item) -> Result[(), E]`; stops at the first callback `Err` |
| `.try_collect()` | `Result[C, AllocError \| E]` | Over `Result[T, E]` items; same targets; first `Err` wins |
| `.sum()` | `Item` | Numeric items; zero when empty |
| `.min()`, `.max()` | `Option[Item]` | Numeric or `str` items; NaN propagates |

For serial pipeline machinery, `collect` and `try_collect` are the only
terminals that allocate. The `try_` prefix means stopping at the first error,
but the error source differs: `try_collect` consumes `Result` items, whereas
`try_fold` and `try_for_each` consume ordinary items and call a fallible callback.
This preserves the contract of the former `std.iter.try_fold`; these callback terminals do not implicitly unwrap `Result` items.

Early-exit terminals drop the remaining state when consuming an owned pipeline.
Through `&mut Iter[T]`, they leave the unvisited remainder with the caller,
just as a borrowed consuming loop does. An item already passed to a callback
or tested by a predicate has been consumed, including the item causing an error.

**Agreed for stage 3:** `.sum()`, `.min()` and `.max()` are terminals. Each
compiles only when the item type supports the operation: `sum` needs numeric
items, `min` and `max` need ordered items. **Implemented** for numeric items
and, for `min`/`max`, `str`; borrowed items are read as `*item`. Tuples are
rejected for now: a tuple containing a float has no NaN rule yet, so they use
`fold` or `reduce`.

- `sum()` returns `Item`; an empty pipeline gives zero. Integer overflow is a
  runtime error, as for `+`.
- `min()` and `max()` return `Option[Item]`, `Nothing` when empty. Ties keep the
  first item.
- NaN propagates. For `sum` this is ordinary float addition. For `min` and
  `max` it is an explicit rule, IEEE 754-2019 `minimum`/`maximum`: any NaN
  item makes the result `Some(NaN)`, and `-0.0` orders below `0.0`. A plain
  `<` loop would instead drop or keep a NaN depending on its position.
  `std.math.min`, `max`, `min_all` and `max_all` follow the same rule.
- There is no parameter to ignore NaN. Filter first; the choice is then
  visible at the call. Separately named NaN-ignoring terminals can be added
  if filtering proves too common or slow.

NumPy, Julia, Java and IEEE 754-2019 also propagate by default; where both
behaviors exist, most libraries use separate names rather than a flag.
`median` is not planned: an exact median stores every item, and a streaming
estimate is a different contract.

### Choosing the collection

There is one collecting terminal. Its target comes from the expected type, as
empty displays already do.

**Hard rule: `collect()` and `try_collect()` build the collection named by
their expected type. Without an expected collection type, they are a compile
error.** There is no default.

```text
names: list[str] = iter(&people).map(def(p: &Person) -> str: copy(&p.name)).collect()?
tags: set[str] = iter(&posts).flat_map(post_tags).collect()?
index(iter(&people).map(def(p: &Person) -> (str, i64): (copy(&p.name), p.id)).collect()?)  # def index(by_name: dict[str, i64])
n = iter(&xs).collect()?.len()
# error: collect() needs an expected collection type; annotate the binding,
#        as in `name: list[i64] = ...collect()?`
```

**Expected-type sites.** The same sites that type an empty display today
(verified with the current compiler):

- an annotated binding, `x: set[u64] = ....collect()?`;
- assignment to a binding or field whose type is known, `xs = ....collect()?`;
- a non-generic parameter, `take(....collect()?)`;
- a function's declared return type, through its `Result`;
- a class field in a constructor call, `Bag(items=....collect()?)`.

The expected type passes through `?` and through the `Result` of the return
type, exactly as `name: list[i64] = []?` does now. Sites that give no
expected type: an unannotated binding, a generic parameter position (as
[type argument inference](../design/language/07-generics/inference.md) already
states), a method receiver (`....collect()?.len()`), and later uses of a
binding. The checker does not look ahead.

**Precedent.** The checker already types expressions from an expected type:

- `xs: list[u8] = [1, 2]?` types the literals as `u8`; parameters do the same
  for numeric literals.
- Empty displays take their element types from the expected `Result` type, and
  `s = []?` with no expected type is an error: "empty collection needs a type;
  annotate the binding, as in `name: list[i64] = []?`, or use a typed
  constructor".
- `Atomic(value)` takes `T` from the expected type, and expected types allow
  anonymous-sum injection and widening.

`collect()` extends this by letting the expected type choose the collection
*kind*, not only its element type. Displays never needed that, because their
syntax picks the kind. The checker change is to treat `collect`/`try_collect`
as a contextually typed terminal in the same expected-type pass.

**Target rules.**

- `list[T]`, `set[T]` and `dict[K, V]` are accepted. Any other expected type,
  is an error naming the accepted kinds; aliases resolve first.
- The element type must equal the item type exactly. Items are already typed
  by the chain (callback types are explicit), so nothing flows backwards into
  the pipeline and there is no conversion: `x: list[u8]` over `i64` items is a
  mismatch, not a cast.
- `set[T]` needs hashable items and keeps the first occurrence, as
  `set[T].from` does. `dict[K, V]` needs owned `(K, V)` pair items, and a later
  duplicate key overwrites, as dict displays do.
- The `Result` wraps the target: `Result[C, AllocError]`, and for
  `try_collect` over `Result[T, E]` items, `Result[C, AllocError | E]`. The
  expected type may be the bare `C` (with `?`) or the whole `Result`.

**Why an error, not a list default.** Numeric literals default to `i64`, but
the closer precedent is the empty display, which errors without context. A
default would make `xs = ...collect()?` silently a list and change meaning
when someone later adds an annotation elsewhere, and readers would need to
know the default to read unannotated code. The error costs one annotation,
which also documents the collection kind at the call site, where readers need
it. It is the rule with no exception: every `collect()` has a visible or
declared target type.

**Alternatives considered.**

- *Passing the container type*, `.collect(set)`, as excitertools does. Python
  has no static types, so the container must be a runtime argument there. In
  Plenty the element type is already known and the target is almost always
  declared nearby; a type-valued argument would be a new kind of argument
  (types are not values) and would duplicate the annotation. It also would
  not remove the need to check it against an expected type.
- *Separate terminals* (`collect_set()`, `collect_dict()`). Explicit at the
  call, but three names for one operation, and an annotation would still be
  checked against them.
- *A type argument*, `collect[set[str]]()`. Turbofish; rejected.

`list[T].from(pipeline)` is not accepted; its diagnostic points to
`collect()`, so there is one spelling.

### Errors

Closure bodies never return from the enclosing function. A `?` inside a callback
returns from the callback, as it does today, so a fallible step produces
`Result` items. Handle them with `try_collect`, which stops at the first
`Err` and drops the rest of an owned pipeline, or with a `for` loop whose body uses
`?`:

```text
parsed: list[i64] = iter(&lines).map(def(l: &str) -> Result[i64, ParseError]: parse_int(l)).try_collect()?
```

`try_fold` and `try_for_each` instead propagate their callback's first error;
handling a `Result` item inside that callback must be explicit. `try_collect`
widens its item error with `AllocError` exactly as anonymous sums widen today.
The callback terminals add no allocation error of their own. There is no
implicit error collection and no hidden recovery.

### Allocation

A serial pipeline allocates nothing of its own: no iterator object, no frame, no item
buffer, no closure environment beyond the inline one. Only the `collect`
family allocates, and it returns `Result`. Callback bodies and source
expressions keep their own contracts. `collect` reserves capacity
when the pipeline's length is known exactly (a range or collection source with
only `map`, `enumerate`, `zip` or `take`), and otherwise grows like a
comprehension.

These guarantees follow the reference's
[allocation boundaries](../design/language/08-errors/02-allocation-and-traps.md):
user callbacks, source expressions and destructors retain their own allocation
behavior. Explicit generator conversion introduces an inline frame. Parallel
execution is a separate boundary: job cells, output buffers and reduction
scratch storage can allocate even for a non-collecting terminal, and failures
must be reported through its parallel error type.

### Ownership and cleanup

- The pipeline owns its owned source, its closures and adapter state (counters).
  Dropping an unconsumed pipeline drops them in reverse creation order without
  running any callback or advancing any generator.
- A borrowed source's loan lasts until the pipeline is consumed or dropped, and
  protects the collection exactly like a `for` loop's loan.
- Closures may capture by value, `mut`, `&` or `&mut`; the captures' loans last
  as long as the pipeline. Two adapters may not both hold `&mut` to the same
  binding, by the ordinary aliasing rule.
- `break`, `return` and `?` in a consuming loop drop the remaining source,
  pending items and closures when the loop owns the pipeline, as loops over
  owned generators do today. A consumer borrowing `&mut Iter[T]` instead
  releases its local state and loan, leaving the owner's remaining pipeline
  available; early termination does not destroy the caller's source or closures.
- `def once` closures are rejected as adapter callbacks, since adapters call
  their callbacks repeatedly. A terminal that calls its callback at most once
  (none in the core set) could accept them later.

## Efficiency model

### Fusion at the consumer

A pipeline value is a compile-time plan: a source descriptor plus an ordered
list of adapter steps, each with its inline closure environment and any counter
slots. Nothing about the plan exists at run time except those slots.

When the plan reaches a consumer, the lowering in `op.rs` expands it into the
same loop structure it already produces for `for` and comprehensions:

- the source becomes its existing loop plan: a counted loop for ranges, an
  indexed loop for lists, a hash-table walk for dicts and sets, a resume call
  for generators;
- `filter` and `take_while` become a conditional `continue` or `break`;
- `map` becomes a call whose result replaces the item;
- `enumerate`, `take` and `skip` become counter slots with checked arithmetic;
- `zip` advances a second source plan in the same loop, as it does today;
- `chain` becomes two loops sharing one outlined body, so the body is not
  duplicated;
- `flat_map` becomes a nested loop;
- the terminal or loop body is the innermost block.

This is how comprehensions already work, so the work is mainly generalizing
an existing plan, not writing a new optimizer. Comprehensions become a
spelling of a pipeline (see [below](#for-loops-comprehensions-and-generator-expressions)).

### Callback calls

A named function is called directly. A closure literal written inside the
chain is known at the call, so its one-expression body is lowered in place,
exactly like a comprehension's element expression, with its captures resolved
to the pipeline's environment slots. A closure held in a local is called
directly through its known producer with its environment address, with no
indirect call. This removes all runtime dispatch from the hot loop; what
remains is at most one direct call per callback per item, and none for closure
literals.

A general function inliner is out of scope for this proposal. If measurement
shows direct calls to small named functions matter, a size-bounded inliner at
the operation level can follow as a separate B19 item.

### Compile-time cost

Fusion adds no generic instances: an adapter is not a generic function, and a
chain is not a nested type that must be instantiated and then optimized away.
Lowering cost is linear in the number of steps at each consumer. Rust's model
creates one monomorphized type per adapter per chain and relies on LLVM to
inline them away; Plenty would pay that as extra Cranelift functions and still
not get the inlining. A pipeline that is not lent is lowered once, at its
single consumer, because it is affine; a lent pipeline is lowered at each
consuming site and in each callee instance. Apply the existing generator nesting limit (64 steps) to
chain length.

### Verification

Acceptance for the implementation, not optional extras:

1. **Operation equivalence.** Snapshot tests compare the typed operations of a
   pipeline with the hand-written loop it replaces, for each adapter. Use the
   per-function dump being added in issue #360 (`--dump` ops/CLIF/asm).
2. **No runtime dispatch.** A test inspects the CLIF of fused pipelines over
   range, list and borrowed-list sources and asserts there is no call to the
   collection dispatcher and no `call_indirect` inside the loop.
3. **No allocation.** Allocation-checks tests (the `allocation-checks`
   feature) run every non-collecting terminal with allocation disabled.
4. **Benchmarks.** Add the table above as a release benchmark. A pipeline over
   a range or list must be within 10% of the hand-written loop; report the
   generator-source rows separately, since their cost is the generator's.
5. **Compile time.** Add a pipeline-heavy workload to `compile_bench` and check
   that cold builds do not regress against the equivalent comprehension code.
6. **Semantics.** Ordering of callback side effects, cleanup on early exit,
   loan errors for borrowed sources, partial consumption through `&mut Iter[T]`
   followed by collection in the caller, and every rejected placement (return,
   pass by value, store, capture, `yield`) with specific diagnostics.
   Cover owned generator conversion and rejection of borrowed conversion,
   `flat_map` returning `Iter`, and the distinct callback-error and item-error
   terminal contracts. For parallel stages, verify tree shape across worker
   counts and chunk boundaries, earliest-input error selection, and draining
   accepted work on early termination and infrastructure failure.

## For loops, comprehensions and generator expressions

- `for x in pipeline:` is the main consumer. `for x in iter(values):` is valid
  but redundant; prefer `for x in values:`. A lint can suggest the shorter form.
- A list comprehension `[f(x) for x in s if p(x)]` lowers to the same plan as
  `iter(s).filter(p).map(f).collect()` into a `list` target. Comprehensions stay: they are the
  Python-shaped way to build a collection, and the tutorial teaches them first.
  The two forms must produce the same operations; that is a test, not a goal.
  Implemented for one `for` clause with any `if` clauses, for lists, sets and
  dictionaries; several `for` clauses keep the nested loop.
- `.generator()` converts an eligible prepared pipeline into one frame. Ordinary
  generator expressions keep their existing syntax and owned-source rules;
  an unconverted pipeline is not a valid generator-expression source. Stored
  range sources already use counted iteration (issue #371).
- `any(xs)`/`all(xs)` over booleans stay as builtins; `.any(keep)`/`.all(keep)`
  take a predicate. The free forms are defined as the pipeline forms with an
  identity predicate.

## Parallel pipelines

`.par(&pool)` marks the boundary: steps before it run serially on the calling
thread to produce input items; steps after it run on the pool's workers. The
pool is the existing `ThreadPoolExecutor`; there is no global pool, so thread
creation stays explicit and fallible, as the
[parallelism proposal](memory-parallelism-and-simd.md#threads-channels-and-parallel-collections)
requires.

```text
with ThreadPoolExecutor(8, 64)? as pool:
    lengths: list[i64] = iter(paths).par(&pool).map(file_size).collect()?
    longest = iter(range(1, n)).par(&pool).map(score).reduce(best)?
```

Stage 4 contract, extending current `pool.map`/`pool.reduce_tree`:

- **Sources.** Owned lists and integer ranges, as `pool.map` accepts today.
  Borrowed lists follow once scoped pool jobs can borrow (B22).
- **Allowed steps after `par`.** `map` and `filter`; `enumerate` must be applied
  before `par`. Order-dependent steps (`take`, `skip`, `take_while`, `chain`,
  `zip`, `flat_map`) are rejected after `par` with a diagnostic naming the step.
- **Callbacks.** Statically named functions or reusable immutable closures that
  wholly own their captures and pass the existing worker eligibility and effect
  checks. `mut`, `&mut` and one-shot captures are rejected; shared borrows wait
  for scoped jobs. Generator sources are rejected, as frames are not
  worker-eligible.
- **Terminals.** `collect` and `try_collect` into a `list` target (lowest-index error wins, as
  `map_result`), `count`, `any`/`all`, and `reduce`. A parallel `reduce` uses
  the fixed adjacent-pair tree of `reduce_tree`, which reassociates; it is
  deterministic for a given input but differs from the serial left fold for
  non-associative operations such as floating-point addition. There is no
  parallel `fold`: it would need an identity and associativity the type system
  cannot check. `for` loops and `for_each` are rejected after `par`, since
  their bodies would run concurrently.
- **Reduction tree.** Preserve the exact
  [fixed-tree reduction contract](../design/facilities/03-concurrency/parallel-reduce.md):
  adjacent pairs in each round, carrying an odd final item forward. After
  filtering, use the retained items in input order. Worker count and chunk
  boundaries must not change that tree; independently reducing each chunk and
  then combining chunk results is not equivalent. Empty input yields `Nothing`.
- **Early termination and effects.** Follow the
  [fallible mapping contract](../design/facilities/03-concurrency/parallel-map.md):
  `try_collect` selects the lowest original input index's worker error, including
  across chunks. An earlier accepted input's error takes precedence over a later
  submission failure. Accepted work is drained before returning, and partial
  outputs are dropped exactly once. For `any`/`all`, a decisive result may stop
  further submission, but already accepted chunks may still run callbacks;
  their effects are not undone. These terminals do not promise the serial
  callback count. An infrastructure failure already encountered is still
  reported, rather than hidden by a decisive boolean result.
- **Result type.** Parallel terminals return `Result[_, PoolMapError]` or
  `ParallelError[E]`, because submission can fail and the pool can be shut down.
- **Granularity.** Each job runs the fused `map`/`filter` body over a
  contiguous chunk of input, not one item. The default number of chunks is the
  number of logical cores. Chunk boundaries depend only on input length and
  that count, never on scheduling, and filtered chunks are concatenated in input
  order. Tuning (explicit chunk sizes, `par_chunks`, adaptive splitting) is
  deferred until stage 4 has benchmarks.

Compile-time checks the type system must enforce: worker eligibility of every
callback and item type (existing closed-world check), no shared mutable
borrows, no order-dependent step after `par`, and `reduce` only (not `fold`)
for parallel combination. Associativity itself cannot be checked; the
documentation states the tree shape, as `reduce_tree` does now.

### Automatic parallelization

Pipelines give B24 a natural unit: a fused loop body whose callbacks are pure
by effect analysis, over a sized source. B24 remains deferred, and this proposal
does not change that: a pipeline without `.par` is always serial. Explicit
`.par` gives the evidence the parallelism proposal asked for: which workloads
benefit and whether effect checks are sufficient.

## Migration

**Status: complete for stage 2.** `std.iter` was removed outright instead of
deprecated: it had no users outside the repository's tests, and there is no
deprecation-warning mechanism. Its tests now exercise fluent generator
pipelines and `std.error.map_error`. The tutorial lesson moved to
[Transform a stream of values](../tutorial/10-functions/streams.md), after the
borrowed-collection pipeline lesson. The items below record the original plan.

- **`std.iter`.** Mark `map`, `filter`, `take`, `fold` and `try_fold`
  deprecated in a deliberate compatibility change after migration review.
  Fluent methods plus `.generator()` now cover their cross-function use cases;
  keep existing functions working until removal is separately approved. Internal uses are limited to
  `tests/test_std_collections.rs` and `tests/test_trap_locations.rs`.
- **Tutorial.** Rewrite
  Transform a stream of values (then in the modules chapter)
  around pipelines (implemented, including explicit returning conversion).
  A later organization pass can move it out of the modules chapter, since `iter` needs no
  import; it belongs after comprehensions and closures in the iteration
  lessons. Review lessons that build lists by looping only to fold them, and the
  comprehension lessons for the equivalence note.
- **Reference.** Add a pipeline page under
  [iteration](../design/language/09-iteration/index.md); update
  [adapters](../design/language/09-iteration/04-adapters.md) (enumerate/zip
  become pipeline starts), the `std.iter` page, `std.math` (its list helpers
  stay; pipelines use the stage 3 terminals), parallel mapping, the cheat sheet and
  `support.md`.
- **Compatibility.** No existing program changes meaning: `iter` is a new name
  (a user function named `iter` keeps priority, as with `enumerate`), and
  `enumerate`/`zip`/`reversed` loops lower as before.

## Alternatives considered

**Method syntax for `std.iter` (extension methods or uniform call syntax).**
Lets `numbers(3).map(sq)` call `std.iter.map`. Cheap to add, but keeps the 9x
per-item cost and the generator-only, owned-capture limits, and an import that
adds methods contradicts the protocol rule that imports never add methods.

The related [protocol method bodies proposal](protocol-method-bodies.md)
distinguishes provided methods selected through a bound or protocol qualification
from automatic method injection. Its agreed, unimplemented design preserves
that import rule. Provided bodies could share adapter implementation logic,
but do not by themselves add fluent methods to built-in generators or remove
their frame and dispatch costs.

**Rust's model: generic adapter structs plus inlining.** Requires storing
generator frames and closures in generic class fields (rejected today),
user-implementable iterator protocols with associated types (unsupported), and
a function inliner in the Cranelift pipeline. It multiplies generic instances
per chain and raises compile time, against a design goal, for performance the
compiler would still have to recover.

**Faster generators only.** Inline the runtime dispatch, call resume directly
when the producer is known, and use a jump table for state. Worth doing for
named generators regardless (a separate B19 item), but a resume per adapter per
item, with frame-backed locals, cannot match a fused loop, and it does not lift
the borrowing limits.

**Comprehensions only.** Comprehensions already fuse, but cannot express
`take`, `take_while`, `chain`, early-exit reductions, or `par`, and nested
clauses become hard to read beyond two levels. The user request is precisely
for the chaining form.

**First-class storable pipelines from the start.** Convenient, but storing a
borrowed pipeline needs stored references (B11), and storing an owned one needs
frame and closure storage in generic data. Keeping pipelines in their creating
scope, with lending for helpers, ships the useful part now.

**`std.math` helpers over lent pipelines.** `sum_iter(&mut pipeline, 0)` and
`fold(0, math.max)` were considered for stage 3 and deferred. They need module
functions as values and generic function values without explicit type
arguments, and they cannot reject unsuitable item types at the call. The
terminals above replace them for pipelines.

## Open questions

1. **User sources.** A user iterator protocol (`next(self: &mut S) ->
   Option[T]`) could make classes pipeline sources once B12 settles; until then,
   a method returning a generator works.
2. **Closure parameter inference.** Should callbacks later omit parameter
   types when the adapter knows the item type (B16)? This would shorten chains
   at the cost of the readability described in
   [closure parameter types](#closure-parameter-types). Decide after stage 1
   with real code to compare.
3. **Passing pipelines by value.** Whether helpers that add steps
   (`def evens(p: Iter[i64]) -> Iter[i64]`) are worth weakening the
   never-leaves-its-scope rule. Not planned; revisit only with use cases that
   closure-returning helpers do not cover.

## Staged plan and suggested backlog wording

1. **Stage 1 — serial core.** `iter`, `Iter[T]`, local-only placement,
   `map`/`filter`/`enumerate`/`zip`/`take`/`skip`/`take_while`, two-parameter
   pair callbacks, and terminals
   `fold`/`reduce`/`count`/`any`/`all`/`find`/`for_each`/`collect`/
   `try_fold`/`try_for_each`/`try_collect` (target from the expected type), fused
   at the consumer,
   with the verification suite above. Issue #371, fixed, removed the stored-range
   slow path that would have misled the benchmarks.
   Small scalar leaf callback substitution and SSA scalar captures are also
   implemented; [measurements](../development/profiling.md#serial-pipeline-callbacks)
   cover the original workload. Generator fluent entry and the explicit owned
   `.generator()` bridge also landed independently of composition.
2. **Stage 2 — composition.** `chain`, `flat_map`, closure-literal splicing,
   comprehensions lowered through the pipeline plan, tutorial rewrite and
   `std.iter` deprecation. `chain` and `flat_map` are implemented. Closure
   literals written in a chain are lowered in place for every adapter and
   callback terminal; literals with `?`, `match`, several statements or a
   nested closure, and closures held in locals, keep the call. Single-`for`
   comprehensions lower through the plan and produce the same operations as
   the equivalent `filter`/`map`/`collect` chain; multi-`for` comprehensions
   and generator expressions keep their own lowering. **Complete:** the
   tutorial teaches pipelines, and `std.iter` was removed rather than
   deprecated (see [Migration](#migration)).
3. **Stage 3 — lending.** `&mut Iter[T]` parameters specialized per pipeline
   type, `next(p)`, resumable slot state for every adapter, store-back only for
   lent pipelines, and partial consumption followed by collection in the
   caller. Measure instance counts in `compile_bench`. The `sum`, `min` and
   `max` terminals described under [Terminals](#terminals) are implemented.
   Every adapter is resumable, and `next(p)` works in the creating scope: a
   later loop or terminal continues after the items taken. In that scope the
   plan's state already lives in the creator's slots, so resuming needs no
   store-back; `take_while` and `zip` record that they have ended so a later
   consumer does not pull past the end. Lending itself is not implemented.
4. **Stage 4 — parallel.** `.par(&pool)` with chunked fused jobs (one chunk
   per logical core by default), the allowed-step rules, and
   `collect`/`try_collect`/`reduce`/`count`/`any`/`all`.

Suggested backlog row: **B75 — Fluent iterator pipelines.** "`iter(source)`
pipelines with chained adapters and terminals, fused at the consumer into one
loop with no adapter dispatch or serial machinery allocation outside collecting terminals.
Stages: serial core, composition and `std.iter` deprecation, lending as
`&mut Iter[T]`, explicit `.par(&pool)`. Depends on issue #371. Relates to B12,
B22, B24, B49. Design: `proposals/fluent-iterators.md`."

## Issues found

- Issue #371: generator expressions over `range(...)` and stored ranges
  iterate through the runtime, about 15x slower than a direct range loop.
  Fixed by local PR #436: they now compile to counted loops.
