# Six rules for a consistent core

> **Status: Accepted; partially implemented.** The [backlog](../backlog.md)
> tracks remaining scope. The evidence below records research against local
> `main` at `b4580c2d` on 2026-10-31, with probes on the debug compiler built
> that day. The evidence is the 2026-10-31 language consistency survey,
> recorded as issues #612–#637. Accepted and scheduled as backlog rows
> B88–B93 ([staged plan](#staged-plan-and-suggested-backlog-wording)); the
> review [decisions](#decisions) settle five questions.
> A seventh rule from the same survey, a written convention page for the
> standard library, is separate documentation work and is only referenced here.

## Summary

The survey found a coherent core with local exceptions. Each exception has
its own explanation, and a learner has to memorise them one at a time. This
proposal replaces about twenty such exceptions with six rules:

1. **One copy rule.** Tuples, built-in types and function values copy when
   their parts copy. A declared class or enum copies only when it says
   `@copy`. The rule is stated in the same words in the reference, the cheat
   sheet and the agent skill.
2. **One formatting path.** `print(x)` writes what `f"{x}"` writes, and
   `str.repr(x)` writes what `f"{x!r}"` writes. Both use a type's `display`
   and `debug` methods, at the top level and inside aggregates.
3. **An owner converts to its view.** Where a view type (`&[T]`, `&substr`) is
   expected, a reference to the owner (`&xs`, `&s`, `b"..."`) is accepted
   without `[:]`. Every byte owner and byte view is a byte source.
4. **Adapters belong to iterators.** `iter(source)` accepts every `for`
   source and gives the same adapters and terminals. Only iterators
   (pipelines and generators) have adapter methods; collections, ranges,
   views and the free sources do not.
5. **An error type names only what can fail.** Allocation-only failure is
   `AllocError`. A domain error type carries allocation as
   `Allocation(AllocError)`. Invalid arguments follow one four-part policy.
6. **Every callable form is a value wherever a callback is accepted.** Named
   functions, closures with or without captures, stored or literal, variant
   constructors and class constructors all pass to the same parameter, and a
   callback behaves the same however it is written.

Each rule is a change to behaviour or teaching, so each has its own backlog
row and can ship independently. The rules do not depend on one another,
except that rule 5 decides the error type that rule 2's `print` reports.

## How to read this page

Each rule has the same sections: what happens today, the proposed rule,
alternatives, cost, what a learner no longer has to learn, and phasing.
"Today" claims were checked against the reference pages linked in the text
and, where marked *probe*, with `plenty --check` or a run on the 2026-10-31
debug build.

Earlier changes removed features rather than deprecating them (`std.iter` was removed in favour of `iter()`; see
[fluent iterator pipelines](completed/fluent-iterators.md)). This proposal follows that
precedent: a breaking change ships with a diagnostic that states the new
spelling, and the same change updates std, tests, the tutorial and the
reference. [Package language editions](language-editions.md) are not
implemented and are not required.

## Rule 1: one copy rule

### Today

The [bindings, moves and copies](../design/language/05-ownership/01-bindings-moves-and-copies.md)
page says scalars, strings, ranges, named function values and "aggregates of
copyable components" copy, and that classes are affine unless declared
[`@copy`](../design/language/03-types/classes.md#copy-classes). "Aggregates"
includes tuples and enums, but the [enums](../design/language/03-types/enums.md)
page never mentions copying. The agent-facing pages each give a different
list (issue #636):

| Source | Says copies |
| --- | --- |
| Reference | Scalars, `str`, ranges, function values, copyable aggregates, `@copy` classes |
| [Cheat sheet](../design/examples/cheatsheet.md) | Scalars, `str`, copyable tuples, `@copy` classes; also says a `@copy` class "cannot take type parameters", which [classes](../design/language/03-types/classes.md#copy-classes) contradicts (`IntType`/`FloatType` bounds are allowed) |
| `plenty` agent skill | Scalars, `str`, `@copy` classes |

None of the agent-facing pages mentions enums, and the rule itself has an
exception: a class must opt in, an enum copies implicitly.

That exception has a cost. Issue #535, which introduced `@copy`, made the
annotation "part of the type's public contract: removing it is a breaking
change". An enum's copyability is the same contract, but it is implied by its
payloads. *Probe:* `enum E: A(i64) | B` copies, so `take(e); take(e)` checks.
Changing the payload to `A(list[i64])` makes `E` affine, and the error appears
at the second `take(e)` in every caller (`use of moved binding e`), not at the
declaration that changed. `@copy enum` is rejected today
(`` `@copy` applies only to class declarations ``).

### Proposed rule

> A value copies when its type is a scalar, `str`, `bytes`, a range, a
> function value, a tuple or built-in generic (`Option`, `Result`) whose parts
> copy, or a class or enum declared `@copy`. Every other value moves.
> `@copy` is checked at the declaration: every field or payload must copy,
> and the type has no `__del__`.

The rationale, stated with the rule: anonymous and built-in types copy by
structure, because they have no author who could intend otherwise. A
declared type copies only when its author says so, because copying is part
of its public contract and must not change when a field changes.

The same sentence goes in the reference, the cheat sheet and the skill. The
cheat sheet's type-parameter claim is corrected. A test compares the three
copies so they cannot drift again.

### Alternatives

- **State today's rule without changing it** (classes opt in, enums by
  structure). This fixes the documentation but keeps the exception and the
  distant-caller breakage. Rejected as the end state; it is phase 1.
- **Make classes structural too, and remove `@copy`.** One rule with no
  annotation, but a class that holds a token, an index or an invariant would
  silently copy, and adding a field would flip the contract of every class.
  This reverses issue #535. Rejected.
- **Allow `@copy` on enums but keep structural copying as the default.**
  The annotation would only add a declaration-site check. Two ways to get the
  same behaviour, and the default still breaks distant callers. Rejected.

### Cost and compatibility

- Phase 1 is documentation and one test.
- Phase 2 (`@copy` accepted on enums) is additive.
- Phase 3 makes undeclared enums affine. std has 158 enum declarations;
  tests, examples and book pages have about 460. A migration script adds
  `@copy` to every enum whose payloads all copy, which preserves today's
  behaviour exactly; reviewers then remove it where an enum should move.
  Generic enums whose copying depends on a type argument need a copyable
  bound, which is B84's open work for generic `@copy` classes. Phase 3
  waits for that bound (decided 2026-10-31), so B84 moves earlier: it now
  gates this rule as well as generic copy classes.
- Copy-dependent behaviour elsewhere (dictionary-key rules in
  [user type hashing](user-type-hashing.md), by-value receivers) keys off
  "copies", so it follows the new rule without its own change.
- The use-after-move diagnostic for an affine enum names the declaration
  and suggests `@copy` when its payloads copy.

### What a learner no longer has to learn

Which kinds of declaration copy implicitly, and why an enum change broke a
caller. The question "does this copy?" has a two-line answer.

### Phasing

1. One statement in all three places; fix the cheat sheet (issue #636).
2. Accept and check `@copy` on enums.
3. Undeclared enums move; migrate std, tests and the book.

## Rule 2: one formatting path

### Today

The [formatting](../design/facilities/01-builtins/formatting.md) page says
`print` and `str.repr` never call user formatting methods. The
[format strings](../design/language/04-expressions/format-strings.md) page
says a field without a conversion uses a public `display` method, `!r` uses
`debug`, and also that "a field is written as `print` writes it", which is
false for such types (issue #615). Inside aggregates, neither path calls user
methods. *Probe*, for a public class `P` whose `display` writes `<3>` and
`debug` writes `P#3`:

| Expression | Output |
| --- | --- |
| `print(p)` | `P(x=3)` |
| `str.repr(p)?` | `P(x=3)` |
| `f"{p}"?` | `<3>` |
| `f"{p!r}"?` | `P#3` |
| `print(ps)`, `f"{ps}"?` for `ps = [p]` | `[P(x=3)]` |

A non-`pub` class with `display` cannot appear in an f-string in its own
module; the diagnostic names `__plenty_fstring_…` (issue #615). `main`'s
error report originally used a third hook, `std.report.Formattable.format`,
over a different `Formatter` type, and otherwise the `str.repr` spelling.
Issue #634 subsequently unified the formatter, error, and reporting protocol in
`std.fmt`; choosing `display` for entrypoint reports remains separate work. See
[entrypoint reporting](../design/toolchain/01-building/entry-report.md).

### Proposed rule

> A value has two spellings. `display` is the default: `print(x)`, `f"{x}"`
> and `main`'s error report write it. `debug` is the precise one:
> `str.repr(x)`, `f"{x!r}"` and the elements of a formatted aggregate write it.
> A type without `display` uses its `debug` spelling; a type without `debug`
> uses its structure.

So `print(p)` writes `<3>`, `print(ps)` writes `[P#3]`, as Python's
`print([p])` uses each element's `repr`. Strings stay raw at the top level
of `print` and quoted inside aggregates, which is today's behaviour and is
the same rule (`display` of `str` is its text, `debug` is quoted).

A type's own methods are found from its defining module whatever its
visibility, so a private class formats in its module. Internal names never
appear in diagnostics.

### Alternatives

- **Make f-strings ignore `display`, as `print` does.** Consistent, but
  removes the only user formatting hook from core syntax and breaks std types
  (`std.path`, `std.complex`, `std.error` and others) that define `display`.
  Rejected.
- **Keep the paths separate and fix the sentence in format-strings.md.**
  Documents the difference instead of removing it; `print(p)` and `f"{p}"`
  still disagree. Rejected.
- **Use `display` inside aggregates too.** Ambiguous for strings and for
  types whose display text contains separators; Python's `str`/`repr` split
  exists for this reason. Rejected.

### Cost and compatibility

- `print` and `str.repr` start running user code. That code can fail with
  `FormatError`, so their result types follow rule 5: unchanged for values
  with built-in spellings, widened with `FormatError` when a user method can
  run. Programs that print such types and return `Result[(), IoError]` need
  the wider error; the diagnostic names the type whose method can fail.
- Output changes for every printed value of a type with `display` or `debug`.
  Expected-output fences in the tutorial and reference that print std types
  with these methods change; the test suite finds each one.
- `main`'s error report uses `display`. `std.fmt.Formattable` becomes a
  transitional fallback while the entrypoint hook migrates. Issue #634
  delivered the shared formatter; the meaning of a method named `format`
  belongs to the std conventions page (rule 7). This rule fixes which hook
  core formatting calls.
- Automatic formatting still rejects recursive types. Each user method an
  aggregate element calls gets its own `std.fmt` formatter with a fresh depth
  of 32 and no byte budget of its own; the methods do not share one depth
  count across the aggregate. The `print` or `str.repr` buffer grows with
  checked allocation as before.

### What a learner no longer has to learn

Which of three output paths calls which hook, and why `print` and an f-string
disagree. One rule: `display` for people, `debug` for precision.

### Phasing

1. Fix the false sentence and the private-class lookup; hide internal names
   (issue #615).
2. Route `print`, `str.repr` and aggregate elements through `display`/`debug`.
3. `main`'s report uses `display`; its shared formatter dependency is delivered
   by issue #634.

## Rule 3: an owner converts to its view

### Today

> **Update 2026-11-01:** phases 1 and 3 shipped. Owners convert to views at
> view types, byte-reading std APIs take `&[u8]`, and `ByteSource` is removed;
> naming it is a compile error pointing to `&[u8]`. The remaining
> `&Bytes, start, stop` inputs are listed under
> [std conventions](../design/stdlib/conventions.md#bytes). The probes below
> record the state before phase 1.

[Slice views](../design/language/04-expressions/slices.md#slice-views) give
`&[T]` for a list, a `Bytes` buffer and a `bytes` literal, so "one `&[u8]`
parameter reads any of them". But "a list is not converted to a view
implicitly; pass the whole list as `&xs[:]`". *Probes:*

| Call | Result |
| --- | --- |
| `total(&xs)` for `total(values: &[i64])` | Rejected: pass `&xs[:]` |
| `n(b"lit")` for `n(t: &[u8])` | Rejected: pass `&xs[:]` |
| `n(&s)` or `n("lit")` for `n(t: &substr)` | Rejected: pass `&s[:]` |
| `s.startswith(w)` for `w: &substr` | Rejected: `expected str, got &substr` |
| `w.upper()` for `w: &substr` | Rejected; the diagnostic lists the dozen methods a `&substr` has |
| `n(&xs)` for `S: ByteSource` and `xs: list[u8]` | Rejected: protocols require classes, enums or `bytes` |
| `n(&xs[:])` for the same | Rejected, and the diagnostic names `Result[list[u8], AllocError]`, the copying slice's type |

`ByteSource` has two
implementations, `Bytes` and `bytes`; a `&[u8]` view and a `list[u8]` are not
among them, although `&[u8]` already unifies all three owners. Byte APIs
therefore take `source: &S, start, stop` triples that a view expresses
directly.

### Proposed rule

> Where a view type is expected, a reference to an owner of that view is
> accepted: `&xs` for `&[T]`, `&mut xs` for `&mut [T]`, `&buf` or `b"..."`
> for `&[u8]`, `&s` or a literal for `&substr`. The conversion is free: it
> creates the same view as `[:]`, with no copy and no allocation. Every
> operation that reads text accepts `&substr`; every operation that reads
> bytes accepts `&[u8]`.

`&xs[:]` keeps working. Expected-type positions are parameters, annotated
locals and returns; a view still cannot be stored (B11).

### Alternatives

- **Keep `[:]` explicit.** The current rationale is that conversions should
  be visible. That matters for costs; this conversion has none, and the
  design goal is "no hidden allocation", not "no hidden pointer
  arithmetic". Rust performs the same coercion from `&Vec<T>` to `&[T]`.
  Rejected.
- **Accept views where owners are expected** (a `&substr` argument for a
  `str` parameter). This would need an allocation or a second representation
  for `str`. Rejected; text-reading parameters change to `&substr` instead,
  which is B12's planned scope.
- **Make `&[u8]` implement `ByteSource`** and keep the protocol. Smaller
  change, but leaves two spellings for "bytes I read". Accepted as the
  first phase only; std then moves to `&[u8]` parameters.

### Cost and compatibility

- Phase 1 is additive: code with `[:]` keeps compiling.
- Phase 2 changes std signatures. Text APIs taking `str` to read change to
  `&substr`; callers passing `str` keep working through the conversion.
  Byte APIs taking `&S, start, stop` change to `&[u8]`, so
  `w.write(&buf, a, b)` becomes `w.write(&buf[a:b])`. The `std.io` `Writer`
  protocol's `write` signature changes, which breaks user writers; the
  [std.io](../design/stdlib/io.md) page and its examples change with it.
  Range-error results on those methods (`BytesError.OutOfRange`) disappear,
  because view bounds clamp.
- `ByteSource` is removed once nothing in std takes it.
- Per-type specialisation of byte generics goes away: one function body
  serves every byte owner. Codegen cost is a pointer and a length either way.
- The misleading `Result[list[u8], AllocError]` diagnostic is fixed in
  phase 1.

### What a learner no longer has to learn

When to write `[:]`, which of `&substr` and `str` a text method wants, and
the `ByteSource` protocol with its start/stop convention. One rule: pass the
owner or a view of part of it.

### Phasing

1. Owner-to-view conversion at expected view types; `&[u8]` satisfies
   `ByteSource`; fix the diagnostic.
2. Text-reading std APIs accept `&substr` (part of B12).
3. Byte-reading std APIs take `&[u8]`; remove `ByteSource`.

`Box` already converts to `&T`, while `Shared` does not (survey section 2).
`&Shared[T]` converts to `&T` as `&Box[T]` does (decided 2026-10-31). The
content is immutable while shared, so the view is sound; the borrow keeps
the handle, and so its count, alive for the view's lifetime. This joins
phase 1.

## Rule 4: adapters belong to iterators

### Today

[Pipelines](../design/language/09-iteration/06-pipelines.md) start with
`iter(source)`. *Probe:* `iter(...)` accepts every `for` source already:
lists, sets, strings, `bytes`, ranges, `reversed`, `enumerate`, `zip`,
`keys()`, `values()`, `items()`, generators and pipelines. Some sources also
take adapters directly and others do not (issue #622):

| Source | Direct `.count()` today |
| --- | --- |
| Generator, `enumerate(&xs)`, `zip(&xs, &ys)`, `d.items()` | Accepted |
| `range(3)` | Rejected: `expected str, got range` |
| `xs` (list) | `xs.map(...)`: `unsupported method map`; `xs.count()`: list `count` needs an element |
| `reversed(&xs)` | Rejected: reversed is supported in loops only |
| `d.keys()` | Rejected; suggests the allocating `keys_snapshot()` |

The three dictionary views differ in kind
([dictionaries](../design/facilities/01-builtins/dictionaries.md), issue #624):
`keys()` is a bindable `KeyView` with `len` and set algebra, `values()` cannot
be bound or measured, and `items()` starts chains but cannot be bound.
Terminals differ by item type: `sum`, `min` and `max` read copyable reference
items through, while `find` and `reduce` reject them (issue #621). `len(x)`
and `in` reject std containers such as `Deque` and `Bytes` that define
`.len()` (issue #623).

### Proposed rule

> A collection, range or view is a *source*. An *iterator* is a single-use
> stream: a pipeline (`Iter[T]`) or a generator. `iter(source)` makes an
> iterator from any `for` source, and `iter(it)` on an iterator returns it.
> Adapters and terminals are methods of iterators only, and every adapter
> and terminal treats items the same way whatever the source.

This matches Python's iterable/iterator split and Rust's `Vec`/`Iterator`
split. It is the only choice with no name clashes: a list already has
`count(e)` and `find`, a string has `count` and `find`, a set has its own
algebra, and none of them is consumed by iterating.

Under the rule:

- `enumerate(...)`, `zip(...)` and `d.items()` stay `for` sources and
  `iter(...)` arguments, but lose direct adapter methods; the iterator
  methods `.enumerate()` and `.zip()` already exist, so
  `enumerate(&xs).count()` becomes `iter(&xs).enumerate().count()`.
- `keys()`, `values()` and `items()` are the same kind of thing: bindable
  borrowed views with `len`, usable in `for`, `in` (keys) and `iter()`.
  `KeyView` is accepted wherever a borrowed set is read.
- `find`, `reduce` and every other terminal copy copyable reference items
  as `min` and `max` do.
- Diagnostics for an adapter on a source suggest `iter(source)`.

### Alternatives

- **Give every source the adapter methods directly.** Clashes with existing
  collection methods (`list.count`, `list.find`, `str.find`), and makes it
  unclear whether a call consumes. Rejected.
- **Remove direct methods from generators too**, so only `Iter` has them.
  A generator is single-use and supports `next`, exactly like a pipeline;
  denying it methods would be a distinction without a reason. Rejected.
- **Fix only the diagnostics.** Issue #622's minimum. Keeps two rules.
  Rejected as the end state; it is phase 1.

### Cost and compatibility

- Direct adapters on `enumerate`, `zip` and `items()` are rare: two uses in
  Plenty code and pages, about thirty in `tests/*.rs` (mostly
  `test_pipelines.rs`). Each becomes `iter(...)` or the iterator method.
- Dictionary-view changes are additive.
- `find`/`reduce` on copyable references is additive.
- `len`/`in` on std containers depends on B49's protocol work; until then
  the diagnostic names `.len()`.

### What a learner no longer has to learn

Which sources take methods directly, three kinds of dictionary view, and
which terminals read through references. One rule: `iter()` first, and
everything that works on one iterator works on all.

### Phasing

1. Diagnostics suggest `iter(source)` (issue #622); `find`/`reduce` copy
   copyable items (issue #621).
2. Uniform dictionary views (issue #624).
3. Remove direct adapters from `enumerate`, `zip` and `items()`.
4. `len` and `in` on std containers, with B49 (issue #623).

## Rule 5: an error type names only what can fail

### Today

Allocation failure has several shapes (issue #632). The table and the probe
describe the state before phasing items 2 and 3. Item 2 made f-strings without
user formatting fail with `AllocError`; item 3 changed the `FormatError`,
`std.bytes` and union rows and `std.bytes.with_capacity(-1)`:

| Operation | Allocation failure reported as |
| --- | --- |
| `[1, 2]`, `a + b`, `copy(x)` | `AllocError` |
| An f-string | `std.fmt.FormatError.Bytes(BytesError.Allocation(...))` |
| `print` | `IoError.Data(DataError.Allocation(...))` ([I/O errors](../design/facilities/02-io/errors.md)) |
| `std.bytes.empty()`, `from_text` | `BytesError.Allocation` |
| `clist.append(row)` | `AppendError[T]`, which returns the row |
| sketch, linalg, mime, graph | A union `E \| AllocError` |

Switching `a + b` to `f"{a}{b}"` changes the function's error type, and
`FormatError` needs an import although every f-string produces it. *Probe:*
`-> Result[str, FormatError]` gives `unknown type FormatError`, and
`-> Result[str, AllocError]` returning `f"n={n}"` is rejected. Tuple slices
return `Result` although tuples never allocate (issue #625).

Invalid arguments follow four policies (issue #629):

| Argument | Today |
| --- | --- |
| `range` step | Compile error unless `NonZero` ([ranges](../design/facilities/01-builtins/ranges.md)) |
| `split("")`, even a literal | Run-time trap ([text splitting](../design/facilities/01-builtins/text-splitting.md)) |
| `repeat(-1)` | Empty result ([text building](../design/facilities/01-builtins/text-building.md)) |
| `list.with_capacity(-1)` | `Err(CapacityOverflow)` ([construction](../design/facilities/01-builtins/construction-copy.md)) |
| `std.bytes.with_capacity(-1)` | `Err(OutOfRange)` |
| `.par(&pool, chunks=0)` | Trap with the unrelated `integer cast out of range` |

### Proposed rule

> An operation's error type names only failures it can produce. If
> allocation is its only failure, the type is `AllocError`. A domain error
> type that can also fail to allocate carries it as one variant,
> `Allocation(AllocError)`. An operation that cannot fail returns no `Result`.

For f-strings: `Result[str, AllocError]` when every field has a built-in
spelling and no spec is computed at run time; `Result[str, AllocError |
FormatError]` when a field runs a user `display`/`debug` or a computed spec
can be invalid. `FormatError` becomes a prelude name. `print` and
`str.repr` follow the same rule (see rule 2). Tuple slices return the tuple.

> **Invalid arguments.**
> 1. A size passed to allocation that cannot be allocated, including a
>    negative one, is `AllocError.CapacityOverflow`.
> 2. Slice bounds and repeat counts clamp, as in Python. This is the
>    operation's defined result, not an invalid argument.
> 3. Any other argument outside its documented domain traps, with a message
>    naming the operation and the argument. A compile-time constant outside
>    the domain is a compile error. Where a type can state the domain
>    (`NonZero` for a range step), the parameter uses it.
> 4. `Err` is for failures the caller cannot rule out by checking its
>    arguments: allocation, I/O, and external data.

So `split("")` with a literal is a compile error and with a variable traps
(as now); `std.bytes.with_capacity(-1)` is `CapacityOverflow`; `chunks=0`
traps naming `chunks`. `repeat(-1)` and slice clamping are unchanged.

### Alternatives

- **Always a union, never a variant** (`IoError | AllocError` everywhere).
  Uniform for matching, but `print` would need `AllocError` in about 390
  `main` signatures that now return `Result[(), IoError]`, and the
  survey's agreed wording keeps domain errors for APIs that have them.
  Rejected. About 42 std error types already use an `Allocation` variant and
  four use a union; the four change.
- **Keep `FormatError` on every f-string and add it to the prelude.**
  Fixes the import but not the signature change from `+` to an f-string,
  which is the more common edit. Rejected; the prelude addition is kept for
  the user-method case.
- **Make every invalid argument an `Err`.** Puts a `?` on calls whose
  failure is a bug the caller could rule out, which teaches ignoring it.
  Rejected.
- **Trap on negative `repeat` counts.** Breaks the padding idiom
  `" ".repeat(width - len(s))` that Python habits rely on. Rejected.

### Cost and compatibility

- f-string signatures: functions returning `FormatError` for f-strings that
  only use built-in spellings must change to `AllocError` (or keep a union
  that contains it). This is the largest edit; it is mechanical, and the
  diagnostic states the new type. Formatting through `out.format(f"...")` in
  `display` methods keeps `FormatError`.
- `FormatError.Bytes(BytesError.Allocation)` becomes
  `FormatError.Allocation(AllocError)`; code matching the nested shape
  changes.
- `std.bytes` constructors return `AllocError`; their callers' signatures
  narrow.
- The four union-style std modules move to an `Allocation` variant. This is
  shared with the std conventions page (rule 7).
- `AppendError[T]` remains: it returns the moved row, a separate convention
  (also issue #630 for `spawn`) that the std conventions page owns.
- Tuple slices: `t[0:2]?` becomes an error with the slice-view wording
  (issue #625).

### What a learner no longer has to learn

Five shapes of allocation failure, an import for f-strings, and four
invalid-argument policies. One rule for each: the type names what can fail,
and an argument bug traps.

### Phasing

1. `FormatError` in the prelude (issue #632); infallible tuple slices
   (issue #625); literal `split("")` rejected and the `chunks` message
   (issue #629).
2. f-strings, `print` and `str.repr` report `AllocError` when no user method
   runs. Done: `print` and `str.repr` with B89 phase 2 (issue #615),
   f-strings with B92 phase 2. A `:c` spec counts as computed, since an
   out-of-range scalar is `InvalidSpec`.
3. std error types: `Allocation(AllocError)` variants, `std.bytes`
   constructors, negative capacities. Done with B92 phase 3:
   `FormatError.Allocation`, `std.bytes` constructors return `AllocError`,
   and `std.graph`, `std.sketch`, `std.linalg`, `std.shlex` and `std.mime`
   (new `MimeError`) carry the variant; `std.actor.stage` returns
   `StageError`. `Bytes` methods that fail only to allocate, and sketch
   `to_bytes`, return `AllocError`. Out-of-domain arguments that depend on
   run-time state (a range beyond a buffer's length, a random index into an
   empty population) still return named `Err` variants: converting them to
   traps needs a function-scope `assert` or a std trap with a message.
   Statically checkable domains (positive capacities and limits) wait for a
   type that expresses them.

## Rule 6: every callable form is a value wherever a callback is accepted

### Today

[Function values](../design/language/06-functions/02-function-values.md) and
[closures](../design/language/06-functions/03-closures.md) give four callable
types, two constraints and three passing modes. Which forms pass depends on
the callee (issues #617–#620). *Probes:*

| Form | Pipeline `filter`/`map` | `sort_by_key` | `f: Callable[...]` | `f: &mut F` |
| --- | --- | --- | --- | --- |
| Named function | Yes | Yes | Yes | No: `named function declarations are immutable` |
| Closure literal | Yes | No: `sort_by_key requires a named function or reusable closure` | Yes, without captures | Needs a binding, as any borrow does |
| Stored closure, no captures | No: `pipeline callbacks require a named function or reusable closure` | Yes | Yes | Yes, in a `mut` binding |
| Stored closure with captures | Yes | Yes | No (code address only) | Yes, in a `mut` binding |
| Variant constructor `E.A` | No | No | Yes | Not probed |
| `Some`, `Ok`, `Err` | Not probed | Not probed | No: `Some` is not a function value | Not probed |
| Class constructor `Point` | Not probed | Not probed | No: `unknown binding Point` | Not probed |

A closure with a `mut` capture passed as `&F` fails at instantiation with
`cannot borrow shared reference as mutable`, without naming the capture
(issue #619). Behaviour can differ too: inside `checked:`, callback overflow
is `Err` for a closure literal and a trap for a named function (issue #614).

### Proposed rule

> Named functions, closures with or without captures (stored or literal),
> variant constructors and class constructors are callable values. Each
> passes wherever a callback with its signature is accepted, and it behaves
> the same whichever form it has.

Concretely:

- Pipelines, `sort_by_key` and every other builtin callback parameter
  accept every form. Each currently accepts a different subset: pipelines
  reject a stored capture-free closure, `sort_by_key` rejects a literal.
- `&mut f` is accepted for a named function, which has no state to mutate,
  so a `&mut F` parameter accepts every reusable callable. `&F` remains the
  default for read-only callbacks; its diagnostic for a `mut` capture names
  the capture and suggests `&mut F`.
- A class constructor `Point` is a function value with the constructor's
  positional signature. A fallible initializer gives a
  `Result[Point, AllocError]` result, as a call does.
- Generic variants, including `Some`, `Ok` and `Err`, follow the rule
  generic functions already follow: an explicit instance (`Option[i64].Some`)
  or a concrete expected `Callable` specialises them. *Probe:* a generic
  `ident[T]` passed to `f: Callable[[i64], i64]` checks today, while
  `Option[i64].Some` is rejected.
- Inside `checked:`, whether overflow is `Err` depends on where the code
  is written, not on the callback's form (decided 2026-10-31, issue #614).
  Arithmetic whose text is in the block, including the bodies of closures
  defined in the block (literal or stored), reports `Err`. Any other
  function, a named function or a closure defined elsewhere, behaves as
  its signature says: an infallible signature keeps trapping, and a
  function that can fail returns `Result`, which the block handles with
  `match` or `?` like any other call. Builtin reductions such as `sum` are
  calls, so their `i64` result traps on overflow, as the
  [pipelines](../design/language/09-iteration/06-pipelines.md) page already
  says; to sum with checking, write `fold` with a closure literal in the
  block. Today a lowered closure literal reports `Err` while the
  [numbers](../design/language/03-types/numbers.md) page says closures
  defined in a block keep trapping; both change to this rule.

### Cost and compatibility

All of rule 6 is additive except two items. The `checked:` fix (issue #614)
changes which programs trap: a closure defined in a block, stored or
lowered, now reports `Err`, while `sum` inside a block, which reports `Err`
today because of pipeline lowering, traps as documented. A closure defined
in a block may not outlive it, since a later overflow would have no block to
exit (decision 5). Constructor values make a class name usable as a value
expression; how that interacts with local bindings of the same name needs a
check against the name-resolution rules in
[names](../design/language/02-source/names.md). Worker-thread effect
certification currently rejects thin function values, including constructor
values (local PR #681); that restriction is about effects, not forms, and
should be stated as such, but it is outside this rule.

### What a learner no longer has to learn

Which callback positions accept which forms, why adding an unused capture
fixes an error, and why `&mut` rejects a named function. One rule: anything
you can call can be passed.

### Phasing

1. Pipelines and `sort_by_key` accept every closure form (issue #617);
   `&mut` named functions and the `&F` diagnostic (issue #619); lead the
   reference with the constraint form (issue #620).
2. Variant constructors in pipelines; class constructors as values; generic
   variants with an expected type (issue #618).
3. Inside `checked:`, code written in the block and closures defined in it
   report `Err`; other functions behave as their signatures say (issue
   #614).

## What the six rules remove

| Rule | Exceptions removed | Issues |
| --- | --- | --- |
| 1 | Three copy lists; implicit enum copying | #636 |
| 2 | `print`/f-string disagreement; private-class f-string failure; third report hook | #615, part of #634 |
| 3 | `[:]` at view parameters; `&substr` rejected by text APIs; `&[u8]` not a byte source | B12 scope |
| 4 | Five sources without methods, three kinds of dictionary view, uneven terminals, `len`/`in` | #621–#624 |
| 5 | Five allocation shapes; f-string import; four invalid-argument policies; fallible tuple slices | #625, #629, #632 |
| 6 | Seven callable forms with different acceptance; form-dependent `checked:` | #614, #617–#620 |

Not covered: the defects in survey section 1 that are bugs, not rules
(issues #612, #613, #616), diagnostics for Python habits (issue #627),
reference diagnostics (issue #628), and std conventions (rule 7; issues
#630, #631, #633–#635). Survey items with existing backlog homes (B16
contextual literals, B36 builtin argument names, B11 borrowed returns) stay
there.

## Decisions

Recorded 2026-10-31 after review:

1. Rule 1: phase 3 waits for B84's copyable bound, and B84 is brought
   forward.
2. Rule 3: `&Shared[T]` converts to `&T`, as `Box` does.
3. Rule 6: `Some`, `Ok` and `Err` follow the generic-function rule; issue
   #556's exclusion is lifted unless implementation shows the fallback is
   needed.
4. Rule 6: `checked:` covers arithmetic written in the block and in
   closures defined in it, whatever their form. Every other function,
   including a named function passed as a callback and builtins such as
   `sum`, behaves as its signature says: it traps, or it returns a `Result`
   that the block handles or propagates with `?`, which also leaves the
   block. A function's possible failures are part of its signature, not of
   the context that calls it.
5. Rule 6: a closure defined in a `checked:` block may not leave it. It
   leaves by assignment to a binding declared outside the block, as the
   block's value or a return value, by being stored in a collection, field,
   `Shared` or channel, or by being passed by value to a function that keeps
   it (`spawn`, `pool.submit`). Passing it as `&f` or `&mut f` to a function
   called in the block is allowed. The checker can treat the closure as
   borrowing a block-scoped value, so existing borrow checking rejects every
   escape; the diagnostic names `checked:` and suggests defining the closure
   outside the block. Rejection can be relaxed later without breaking
   programs; trapping when called outside the block would reintroduce
   behaviour that depends on where a closure is called.

## Staged plan and suggested backlog wording

Each rule's phase 1 is small and independent; the issues listed are the
work items. Later phases change behaviour and need the user's agreement per
row. The "Proposal" links below point into this page; in the backlog they
become `proposals/consistency-rules.md#...`.

| ID | Area | Suggested wording |
| --- | --- | --- |
| B88 | Copy rule | One copy rule in the reference, cheat sheet and skill, checked by a test; `@copy` on enums; undeclared enums affine after a migration that annotates every currently copyable enum. Generic enums wait for B84's copyable bound, so B84 moves earlier. [Proposal](#rule-1-one-copy-rule); issue #636. |
| B89 | Formatting path | `print`/`f"{x}"`/`main`'s report use `display`; `str.repr`/`!r`/aggregate elements use `debug`; private types format in their module. Report hook migration uses the shared formatter delivered by issue #634. [Proposal](#rule-2-one-formatting-path); issue #615. |
| B90 | Owner-to-view conversion | `&xs`, `&s`, `b"..."` accepted at view types; `&[u8]` satisfies `ByteSource`; `&Shared[T]` converts to `&T` like `Box`; then byte-reading std APIs take `&[u8]` and `ByteSource` is removed. Text APIs accepting `&substr` stay in B12. [Proposal](#rule-3-an-owner-converts-to-its-view). |
| B91 | Iteration sources | Adapters only on iterators; `iter()` for every source; uniform dictionary views; uniform terminals over copyable references. `len`/`in` on std containers stays B49. [Proposal](#rule-4-adapters-belong-to-iterators); issues #621, #622, #624. |
| B92 | Error types and invalid arguments | Allocation-only failure is `AllocError`, including f-strings without user formatting; domain errors carry `Allocation(AllocError)`; `FormatError` in the prelude; infallible tuple slices; the four-part invalid-argument policy. [Proposal](#rule-5-an-error-type-names-only-what-can-fail); issues #625, #629, #632. |
| B93 | Callable forms | Every callable form accepted at every callback parameter; `&mut` named functions; class constructors and generic variants, including `Some`/`Ok`/`Err`, as function values; inside `checked:`, closures defined in the block report `Err` and other functions follow their signatures. Callable names stay (issue #24). [Proposal](#rule-6-every-callable-form-is-a-value-wherever-a-callback-is-accepted); issues #614, #617–#620. |
