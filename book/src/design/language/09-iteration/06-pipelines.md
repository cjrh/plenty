# Local iterator pipelines

`iter(source)` starts a lazy, single-use pipeline. It needs no import. Source
expressions and callback captures are evaluated immediately, from left to right;
items are visited only when a loop, comprehension, or terminal consumes it.
The compiler fuses the adapters into the source's ordinary iteration loop.

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

A local may have an `Iter[T]` annotation. Its concrete source and callback
identity remain part of its type. Consuming it, adapting it, or `drop(p)` moves
it; a second use is rejected. Dropping it releases the owned source and
captures without advancing a generator or calling a callback. Early exit from
an owning consumer releases the unconsumed source and captures.

Pipelines cannot be returned, passed as arguments, borrowed, captured, or put in
stored data. They are currently rejected in generator functions, including
before a `yield`. Pipeline lending and explicit conversion to generator frames
are not implemented.

## Adapters

| Method | Behavior |
| --- | --- |
| `map(f)` | Call `(Item) -> U` once per item; emit the returned value |
| `filter(f)` | Retain items for which the shared-reference predicate is true |
| `take(n)` | Emit at most `n` items; zero or negative `n` advances nothing |
| `skip(n)` | Discard the first `n` items; zero or negative `n` skips nothing |
| `take_while(f)` | Stop at the first false shared-reference predicate, dropping that item |

Predicates receive `&T` for owned `T`, borrowed `&T`, and mutable `&mut T`
items. Mapping receives the item itself. Callback parameters and results require
explicit types. Named functions and reusable closures are accepted; captures
may be owned or borrowed. One-shot closures and indirect function values are
rejected. `map` cannot currently return references.

The implemented core handles single item bindings. Pipeline pair callbacks and
chained `enumerate`/`zip` are not implemented; use existing paired loops instead.
There is a 64-adapter limit, alongside the ordinary expression nesting limit.

## Terminals

| Method | Result and behavior |
| --- | --- |
| `fold(initial, f)` | Left fold with callback `(A, Item) -> A` |
| `reduce(f)` | Left fold without an initial value; `Option[Item]`, empty is `Nothing` |
| `count()` | Checked `i64` item count |
| `any(f)` / `all(f)` | Short-circuit shared-reference predicates; empty is false / true |
| `find(f)` | First matching item in `Option[Item]`, then release the rest |
| `for_each(f)` | Call `(Item) -> ()` for each item |

`find` and `reduce` need an item that can be stored in an `Option`; map a
borrowed item to an owned value first. Collecting and fallible terminals are
not implemented in this slice. Use comprehensions to collect, or a consuming
`for` loop with `?` to propagate failures.

## Cost

The pipeline itself adds no heap allocation or per-adapter generator frame.
Source expressions, callbacks, and destructors retain their own costs. A
generator source still pays its normal resume cost. Named callbacks and concrete
closure callbacks are direct calls; closure bodies are not spliced into the
loop yet. Therefore loop fusion does **not** promise the performance of a
hand-written loop containing the callback expressions inline.

The [proposal](../../../proposals/fluent-iterators.md) describes the broader
design; [B75](../../../backlog.md) tracks the remaining stages. `std.iter`
continues to support owned generator composition across function boundaries.
