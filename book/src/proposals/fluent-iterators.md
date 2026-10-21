# Fluent iterator pipelines

**Status: Not implemented.** Current iteration is defined by
[iteration and generators](../design/language/09-iteration/index.md),
[`std.iter`](../design/stdlib/iter.md), [closures](../design/language/06-functions/03-closures.md)
and [ordered parallel mapping](../design/facilities/03-concurrency/parallel-map.md).
Related backlog entries are B12 (iterator/view protocols, borrowed adapters),
B49 (`std.iter`), B22 (broader parallel mapping) and B24 (automatic
parallelization). No backlog item schedules this design yet. Code blocks on this
page are proposed syntax unless they are labelled as current.

## Summary

Add `iter(source)`, which starts a *pipeline*: a compiler-known, lazily
evaluated chain of adapter methods (`map`, `filter`, `take`, ...) ending in a
loop, a comprehension, or a terminal method (`fold`, `count`, `to_list`, ...).

```text
total = iter(&payments)
    .filter(def(p: &Payment) -> bool: p.name == "bob")
    .map(def(p: &Payment) -> i64: p.cents)
    .fold(0, def(acc: i64, cents: i64) -> i64: acc + cents)
```

A pipeline is not a generator and not a library object. The compiler lowers the
whole chain at its consumer into one loop, the same loop a hand-written `for`
or comprehension produces, with each callback called directly. Only terminals
that build a collection allocate, and they return `Result`. A `.par(&pool)` step
moves the rest of the pipeline onto an explicit thread pool, reusing the
existing ordered `map` and fixed-tree `reduce` contracts.

The surface follows [excitertools](https://github.com/cjrh/excitertools): one
wrapper, chained methods, terminal "sinks". The cost model follows Rust's
`Iterator`, but fusion is done by the compiler rather than by monomorphized
adapter structs and an optimizer's inliner, which Plenty does not have.

## What exists today

Plenty has iteration, but no fluent interface and no zero-cost composition.
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

Current `std.iter` usage is nested calls, generator sources only:

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
`for n in r`, takes 4.71 s. These numbers set the target: a pipeline should be
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

Stage 1 uses one hard rule, matching how `enumerate` and `items()` work today:
**a pipeline is a local value that is consumed in the function that creates
it.** It may be:

- a `for` source or comprehension source;
- the receiver of a terminal method;
- bound to a local and later used once in one of the ways above (it is affine);
- turned into an owned generator with a generator expression,
  `(x for x in pipeline)`, when it must cross a function boundary. That
  conversion is explicit and costs what generators cost today.

It may not be returned, passed by value, stored, captured, or held across
`yield`. Because a pipeline never outlives its statement or local, it can hold
borrowed sources and closures with borrowed captures. This removes gaps 2 and
3 without waiting for stored references (B11).

Stage 3 may allow passing a pipeline by value to a generic parameter, with the
consumer specialized per pipeline type (see [open questions](#open-questions)).

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
| `.chain(other)` | none; same item type | `Item` from self, then other |
| `.flat_map(f)` | `(Item) -> source` | Items of each returned source |

Rules that keep callbacks predictable:

- **Predicates always receive a shared reference to the element.** For an owned
  item `T` that is `&T`; for a borrowed item `&T` it is the same `&T`, never
  `&&T`; for `&mut T` it is `&T`. `map` receives the item itself.
- **Reference pairs are spread.** A tuple item containing references
  (`items()`, `zip` or `enumerate` over borrowed sources) is passed to a callback
  as separate arguments, as loops already require two unpacked bindings for
  such items. Owned tuple items are passed as one tuple.
- `zip` stops at the shorter input and tests the left side first, as today.
  `chain` requires both sides to have exactly the same item type.
- `take_while` drops the first rejected item; there is no peeking.
- `flat_map`'s callback must return an owned pipeline source (a list, range,
  generator, or a nested pipeline created inside the callback).

Later additions, each needing a use case: `step_by`, `map_while`, `scan`,
`inspect`, `chunks(n)` and `windows(n)` over borrowed lists (as borrowed
sub-range views, which depend on B11/B39), `dedup`, `rev` for reversible
pipelines, `zip` of more than two sources, `peekable`, `group_by`, and
`cycle`. excitertools' larger surface (`map_reduce`, `send`, I/O sources such as
`Iter.open`) stays out of the language; such sources belong in `std` modules
returning generators.

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
| `.to_list()`, `.to_set()`, `.to_dict()` | `Result[collection, AllocError]` | The only allocating terminals; `to_dict` needs owned pairs |
| `.try_fold(initial, f)` | `Result[A, E]` | Stops at the first `Err` |
| `.try_for_each(f)` | `Result[(), E]` | Same |
| `.try_to_list()` | `Result[list[T], AllocError \| E]` | Over `Result[T, E]` items; first `Err` wins |

There is no `.sum()`, `.min()` or `.max()` method. Numeric helpers belong in
`std.math`; extend `std.math.sum_iter`/`prod_iter`/`min_all`/`max_all` to
accept a pipeline (or add pipeline overloads there), so `sum` keeps one home.
`fold` covers the rest.

### Errors

Closure bodies never return from the enclosing function. A `?` inside a callback
returns from the callback, as it does today, so a fallible step produces
`Result` items. Handle them with a `try_` terminal, which stops at the first
`Err` and drops the rest of the pipeline, or with a `for` loop whose body uses
`?`:

```text
parsed = iter(&lines).map(def(l: &str) -> Result[i64, ParseError]: parse_int(l)).try_to_list()?
```

The `try_` terminals widen with `AllocError` exactly as anonymous sums widen
today. There is no implicit error collection and no hidden recovery.

### Allocation

A pipeline allocates nothing of its own: no iterator object, no frame, no item
buffer, no closure environment beyond the inline one. Only `to_list`, `to_set`,
`to_dict` and `try_to_list` allocate, and they return `Result`. Callback bodies
and source expressions keep their own contracts. `to_list` reserves capacity
when the pipeline's length is known exactly (a range or collection source with
only `map`, `enumerate`, `zip` or `take`), and otherwise grows like a
comprehension.

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
  pending items and closures, as loops over generators do today.
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
not get the inlining. Each pipeline is lowered once, at its single consumer,
because it is affine. Apply the existing generator nesting limit (64 steps) to
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
   loan errors for borrowed sources, and every rejected placement (return,
   store, capture, `yield`) with specific diagnostics.

## For loops, comprehensions and generator expressions

- `for x in pipeline:` is the main consumer. `for x in iter(values):` is valid
  but redundant; prefer `for x in values:`. A lint can suggest the shorter form.
- A list comprehension `[f(x) for x in s if p(x)]` lowers to the same plan as
  `iter(s).filter(p).map(f).to_list()`. Comprehensions stay: they are the
  Python-shaped way to build a collection, and the tutorial teaches them first.
  The two forms must produce the same operations; that is a test, not a goal.
- A generator expression is a pipeline converted to a frame. Its outer source
  should lower through the source plan, not a stored range (issue #371).
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
    lengths = iter(paths).par(&pool).map(file_size).to_list()?
    longest = iter(range(1, n)).par(&pool).map(score).reduce(best)?
```

Stage 4 contract, extending current `pool.map`/`pool.reduce_tree`:

- **Sources.** Owned lists and integer ranges, as `pool.map` accepts today.
  Borrowed lists follow once scoped pool jobs can borrow (B22).
- **Allowed steps after `par`.** `map`, `filter`, and `enumerate` applied
  before `par`. Order-dependent steps (`take`, `skip`, `take_while`, `chain`,
  `zip`, `flat_map`) are rejected after `par` with a diagnostic naming the step.
- **Callbacks.** Statically named functions or reusable immutable closures that
  wholly own their captures and pass the existing worker eligibility and effect
  checks. `mut`, `&mut` and one-shot captures are rejected; shared borrows wait
  for scoped jobs. Generator sources are rejected, as frames are not
  worker-eligible.
- **Terminals.** `to_list`, `try_to_list` (lowest-index error wins, as
  `map_result`), `count`, `any`/`all`, and `reduce`. A parallel `reduce` uses
  the fixed adjacent-pair tree of `reduce_tree`, which reassociates; it is
  deterministic for a given input but differs from the serial left fold for
  non-associative operations such as floating-point addition. There is no
  parallel `fold`: it would need an identity and associativity the type system
  cannot check. `for` loops and `for_each` are rejected after `par`, since
  their bodies would run concurrently.
- **Result type.** Parallel terminals return `Result[_, PoolMapError]` or
  `ParallelError[E]`, because submission can fail and the pool can be shut down.
- **Granularity.** Fusion matters most here: each job runs the fused
  `map`/`filter` body over a contiguous chunk of input, not one item, and
  writes into its slice of the preallocated output. Chunk boundaries are a
  pure function of input length and `max_workers`, so results do not depend on
  scheduling. Filtered chunks are concatenated in input order.

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

- **`std.iter`.** Mark `map`, `filter`, `take`, `fold` and `try_fold`
  deprecated when stage 2 lands, keep them working (they still serve as the
  generator-to-generator route across function boundaries), and remove them
  once no `std` module or test needs them. Internal uses today are limited to
  `tests/test_std_collections.rs` and `tests/test_trap_locations.rs`.
- **Tutorial.** Rewrite
  [Transform a stream of values](../tutorial/07-modules/06-iterator-adapters.md)
  around pipelines. It moves out of the modules chapter, since `iter` needs no
  import; it belongs after comprehensions and closures in the iteration
  lessons. Review lessons that build lists by looping only to fold them, and the
  comprehension lessons for the equivalence note.
- **Reference.** Add a pipeline page under
  [iteration](../design/language/09-iteration/index.md); update
  [adapters](../design/language/09-iteration/04-adapters.md) (enumerate/zip
  become pipeline starts), the `std.iter` page, `std.math` (`sum_iter` and
  friends accept pipelines), parallel mapping, the cheat sheet and
  `support.md`.
- **Compatibility.** No existing program changes meaning: `iter` is a new name
  (a user function named `iter` keeps priority, as with `enumerate`), and
  `enumerate`/`zip`/`reversed` loops lower as before.

## Alternatives considered

**Method syntax for `std.iter` (extension methods or uniform call syntax).**
Lets `numbers(3).map(sq)` call `std.iter.map`. Cheap to add, but keeps the 9x
per-item cost and the generator-only, owned-capture limits, and an import that
adds methods contradicts the protocol rule that imports never add methods.

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
frame and closure storage in generic data. The local-only rule ships the useful
part now and leaves a clear extension path.

**Free `sum`, `min`, `max` methods.** Rejected for consistency with the
`std.math` decision.

## Open questions

1. **Crossing function boundaries.** Should stage 3 allow a pipeline to be
   passed by value to a generic parameter (`def total[P](items: P) -> i64`) and
   returned from a function, with fusion happening in the specialized
   consumer? That is Rust's ergonomics at the cost of one instance per pipeline
   type. Borrowed pipelines could only be passed down, never returned, until B11.
2. **Collect spelling.** `to_list()` versus a typed `collect()`, and whether
   `list[T].from(pipeline)` should also be accepted (two spellings).
3. **Pair callbacks.** Is spreading reference pairs into two callback
   parameters clearer than passing a tuple of references, which Plenty does not
   currently construct?
4. **User sources.** A user iterator protocol (`next(self: &mut S) ->
   Option[T]`) could make classes pipeline sources once B12 settles; until then,
   a method returning a generator works.
5. **Parallel chunking.** Fixed policy versus `.par_chunks(&pool, n)`.
6. **Closure literal syntax.** Pipelines make the full
   `def(x: T) -> U: expr` form verbose. Parameter-type inference from the
   adapter's expected callable (B16) would help more here than anywhere else.

## Staged plan and suggested backlog wording

1. **Stage 1 — serial core.** `iter`, `Iter[T]`, local-only placement,
   `map`/`filter`/`enumerate`/`zip`/`take`/`skip`/`take_while`, and terminals
   `fold`/`reduce`/`count`/`any`/`all`/`find`/`for_each`/`to_list`/`to_set`/
   `to_dict`/`try_fold`/`try_for_each`/`try_to_list`, fused at the consumer,
   with the verification suite above. Fix issue #371 first so generator
   expressions and stored ranges do not mislead the benchmarks.
2. **Stage 2 — composition.** `chain`, `flat_map`, closure-literal splicing,
   comprehensions lowered through the pipeline plan, `std.math` pipeline
   helpers, tutorial rewrite and `std.iter` deprecation.
3. **Stage 3 — boundaries.** Decide open question 1; if accepted, generic
   pass-by-value with specialization.
4. **Stage 4 — parallel.** `.par(&pool)` with chunked fused jobs, the
   allowed-step rules, and `to_list`/`try_to_list`/`reduce`/`count`/`any`/`all`.

Suggested backlog row: **B75 — Fluent iterator pipelines.** "`iter(source)`
pipelines with chained adapters and terminals, fused at the consumer into one
loop with no runtime dispatch or allocation outside collecting terminals.
Stages: serial core, composition and `std.iter` deprecation, generic
boundaries, explicit `.par(&pool)`. Depends on issue #371. Relates to B12,
B22, B24, B49. Design: `proposals/fluent-iterators.md`."

## Issues found

- Issue #371: generator expressions over `range(...)` and stored ranges
  iterate through the runtime, about 15x slower than a direct range loop.
