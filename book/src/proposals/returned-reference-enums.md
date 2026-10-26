# Returned enums that carry references

**Status: Not implemented.** Nothing shipped; this page is the design record
for returning `Option[&T]` and `Result[&T, E]`. The reference remains
authoritative: [returned references](../design/language/05-ownership/04-returned-references.md)
and [borrowing](../design/language/05-ownership/02-borrowing.md) reject every
reference inside an enum payload. No backlog entry exists yet; the work extends
B11 (broader borrowing) and unblocks the getter follow-ups in issue #516
(`std.cache.lru`) and `std.graph`. [Open questions](#open-questions) list the
decisions still needed.

## Problem

"Find it, or say it is absent" is one of the most common things a program
does. In Plenty today a function cannot return a borrow that may be absent:

```text
def find(items: &list[Item], name: &str) -> Option[&Item]:
#                                            ^ references cannot be stored in enum payloads
```

Every container that holds non-copyable values works around this, and each
workaround costs something:

| Workaround | Where it is used | Cost |
| --- | --- | --- |
| Callback getters, `get_with(&key, &f) -> Option[R]` | `std.cache.lru` `get_with`, `peek_with` | A callback per lookup; no `?`, `return` or `break` from inside it; an extra type parameter in every signature |
| Trapping getters plus a membership test | `LruCache.get_ref`/`peek_mut`, `Heap.peek`, `graph.node`/`edge` | Two lookups (`contains` then the getter); a missing key is a crash rather than a value |
| Return an index, then index again | `std.http.find`, `std.http.cookie.find`, `std.http.body.find`, `std.json.find`, `std.toml.find` | A second bounds check; the index is not tied to the collection, and private layouts (LRU slots) cannot expose one |
| Copying lookup | `dict.get`, `list.get` | Rejects affine values ("get cannot return owned dictionary values") |
| Rejected outright | `next(p)`, `find` and `reduce` on pipelines of `&T` items | "references cannot be stored in a find result; map to an owned value first" |

The [lookup lesson](../tutorial/04-collections/lookup.md) currently tells
learners "Optional borrowed lookup is not implemented". `std.graph` traps on an
invalid handle by calling itself with an out-of-range index, because a getter
cannot report absence.

Plenty avoids lifetime annotations with two rules: a returned reference borrows
from the first reference parameter (the *origin*), and references never enter
aggregates. This proposal keeps both. It adds one narrow exception: an
`Option` or `Result` may carry a reference *as a value in flight*, produced by
a call under the origin rule and consumed locally, but never stored.

## What exists today

Probes ran on local `main` at `13579f12` with a debug compiler
(`plenty --check`).

| Probe | Result | Consequence |
| --- | --- | --- |
| `def find(xs: &list[Item], n: &str) -> Option[&Item]` | Rejected: references cannot be stored in enum payloads | The motivating signature |
| `-> Result[&str, i64]`, `o = Some(&xs[0])`, `o: Option[&i64] = Nothing` | Same rejection | Construction and annotation share one barrier (`Ty::contains_reference`) |
| `enum Found: Hit(&str)` | Rejected at the declaration | User enums use the storage-barrier check |
| `-> (&str, i64)`, `t = (find(&xs, "a"), 1)` | Rejected: references cannot be stored in tuples | Tuples are aggregates; out of scope here |
| `Callable[[&str], Option[&str]]` | Same rejection | Callable signatures share the type check |
| `def pick[T](x: T) -> Option[T]` called with `&s` | Rejected: reference type arguments are not supported | A type parameter is never a reference; generic user enums cannot carry one |
| `d.get("a")` with `list[i64]` values | Rejected: get cannot return owned dictionary values | Affine values have no optional lookup |
| `next(p)`, `iter(&xs).find(f)` with `&str` items | Rejected: references cannot be stored in a next/find result | Pipelines hit the same wall |
| Return a `&mut` element on one path, `xs.append(...)` on the other | Accepted and runs | Loans already end per path, so get-or-insert shapes work |
| `def touch(xs: &mut list[i64]) -> &i64`, then read `xs` while the result is live | Accepted | A shared result from an `&mut` origin holds a shared loan |
| Payload reference returned from `match &node.next` | Accepted | Borrowed-match payloads already satisfy the origin rule |

Relevant implementation facts:

- **Representation.** Wide values (enums, classes) and references are 128-bit
  slots. A reference's low word is a slot address; its high word is a *tag
  shift*, non-zero only after projecting into a nested sum payload
  (`src/codegen/references.rs`). Enum tags stack in the high word:
  wrapping is `(payload_tags << tag_bits) | tag`, unwrapping shifts back
  (`src/codegen/enums.rs`, `wrap_tagged`, `tagged_payload`). There is no
  niche optimization.
- **Origins.** The checker picks an origin only when the result type is
  `Ty::Ref` (`src/frontend.rs`, `return_origin`); `check_return_reference`
  compares each return path's loan root with it, and
  `call_reference_result` gives the caller a child loan of argument 0's loan
  (`src/frontend/references.rs`). The ownership pass itself only sees
  `Loan`/`UseLoan`/`EndLoan` ops, so it is type-agnostic.
- **Local loan carriers that are not references already exist.** `KeyView`
  values and `Iter[T]` pipelines hold loans per slot through
  `reference_locals`; neither can be returned.
- **`?` and builtins.** `?` consumes the operand and moves the payload out
  (`lower_try`). `unwrap`, `unwrap_or`, `ok_or`, `is_some` and friends are
  compiler builtins (`src/frontend/collections.rs`), not generic Plenty
  methods; neither `?` nor `unwrap` forwards a loan today. There is no `map`
  or `and_then` on `Option`/`Result`, and no `if let`/`while let` form: `match`
  is the only pattern construct.

## Precedents

| Language | Optional borrow | How it stays sound |
| --- | --- | --- |
| Rust | `Option<&T>`, `Result<&T, E>`, any enum | Lifetime parameters, elided for one input reference; references may be stored anywhere a lifetime allows |
| Swift | No borrowed optionals; `Optional<T>` copies or retains | Reference counting; `inout`/`borrowing` parameters cannot be returned |
| Hylo (Val) | No first-class references; `subscript` yields a projection for a scope | Projections cannot escape their scope |

Rust's lifetime elision rule ("one input reference: the output borrows it") is
Plenty's origin rule. Rust needs annotations only for the cases this proposal
continues to reject: several possible origins and stored references.

## Proposal

### The rule

A **reference-carrying enum** is an `Option` or `Result` type in which a
variant's payload is a reference, directly or through further `Option`/`Result`
payloads: `Option[&T]`, `Option[&mut T]`, `Result[&T, E]`,
`Result[Option[&T], E]`. Its reference payloads behave exactly like a returned
reference, and the value itself is never stored:

1. **Produced by a call or a local construction.** A function or method may
   declare a reference-carrying enum as its return type. Every return path,
   including each `?` that propagates, must originate in the first reference
   parameter under the existing [returned-reference rule](../design/language/05-ownership/04-returned-references.md).
   `Nothing` and payload-free variants originate in nothing and are always
   accepted. A function without a reference parameter cannot declare one, and
   an `&mut` payload requires an `&mut` origin.
2. **Held by a local.** The caller may bind the result to a local, which holds
   the origin loan until the last use of the local *and* of every reference
   extracted from it. `Some(&place)`, `Ok(&place)` and `Err(&place)` may be
   bound to a local or returned; the binding holds that place's loan, as
   `r = &place` does.
3. **Consumed by inspection.** It may be matched, propagated with `?`, tested
   (`is_some`, `is_ok`, ...), unwrapped, printed, or passed straight to `return`.
4. **Never stored.** It cannot enter a tuple, class field, list, dictionary,
   set, `Box`, closure capture, another enum value, a scoped worker, a channel,
   or a live state across `yield`. It cannot be a type argument other than the
   payload position of `Option`/`Result`, so `list[Option[&T]]`,
   `Generator[Option[&T]]` and `Box[Option[&T]]` stay rejected.

Rule 4 is the existing "references never enter aggregates" rule with no
exception: the enum is the reference's wrapper, not a container it is stored
in. A nested type such as `Result[Option[&T], E]` is written in one signature
and produced as one value; wrapping an existing `Option[&T]` local in `Ok(...)`
at a return is permitted for the same reason, and is the only place an enum
value may appear inside another.

```text
class Item:
    name: str
    tags: list[str]

def find(items: &list[Item], name: &str) -> Option[&Item]:
    for item in items:
        if item.name == name:
            return Some(item)
    Nothing

def main() -> Result[(), AllocError | IoError]:
    mut items = [Item("ada", ["x"]?)]?
    match find(&items, "ada"):
        case Some(item):
            print(item.tags)?      # `item: &Item`; `items` is borrowed here
        case Nothing:
            items.append(Item("ada", []?))?   # the loan ended at the match
    Ok(())
```

### Which enums qualify

| Option | For | Against |
| --- | --- | --- |
| `Option` only | Smallest change; covers `get`, `find`, `first` | Fallible borrowed lookups (`Result[Option[&V], E]`, a parser returning `Result[&Token, ParseError]`) remain impossible |
| **`Option` and `Result`, nested in each other (recommended)** | Covers infallible and fallible lookups; both are builtin sums whose methods are compiler builtins, so no generic instantiation with a reference type argument is needed; `?` already treats them specially | Two types to document instead of one |
| Any enum, including user enums | Uniform | User enum payloads come from declarations or type parameters. A declaration naming `&T` would be the first user type with a reference field, which reads as "references can be stored". A generic `Lookup[&V]` needs reference type arguments, which Plenty does not support and which would make every generic body a potential reference holder |

Recommendation: `Option` and `Result` only. User enums keep the rule that
declarations and type arguments never contain references. Revisit only with a
concrete case that `Result[Option[&T], E]` cannot express.

Anonymous error sums (`AllocError | IoError`) are `Result` error types and may
not contain references; an error that borrows its input would escape through
`?` into unrelated callers' error unions.

### Matching, `?` and builtins

- **`match` by value** on a reference-carrying enum binds payload names as the
  reference type itself (`item: &Item`, `slot: &mut V`). Each binding is a child
  of the enum local's loan, like a [borrowed-match payload](../design/language/05-ownership/05-borrowed-matching.md).
  In an arm without payload bindings, the enum's last use is the scrutinee, so
  the loan ends there; this is what lets the `Nothing` arm above mutate `items`.
- **`match &o`** is rejected: a reference to a reference-carrying enum would be
  a reference to a reference, which Plenty does not have. The diagnostic says to
  match `o` directly. Matching a shared `Option[&T]` by value copies only the
  reference and leaves `o` usable.
- **`?`** returns early on `Nothing`/`Err` and yields the payload reference,
  which inherits the operand's loan: `item = find(&items, "ada")?` is a
  reference binding from a call result. Propagation is a return path: in a
  function returning `Option[&T]`, `?` on `Nothing` returns `Nothing`, which
  borrows nothing, and the extracted reference must still reach any returned
  `Some` from the origin.
- **`is_some`, `is_none`, `is_ok`, `is_err`** read the tag; they are uses of
  the value and extend its loan to that point only.
- **`unwrap()`** yields the reference with the operand's loan and traps on
  absence, the same as indexing.
- **`ok_or(error)`** turns `Option[&T]` into `Result[&T, E]` with an owned
  error; the loan carries over.
- **`unwrap_or(default)` with a reference default is rejected.** The result
  could borrow either the option's origin or `default`, which is a conditional
  reference expression; those are unsupported, and supporting them only here
  would give one builtin a rule no function signature can state. The
  diagnostic suggests `match`.
- **Printing** reads through the reference (`Option[&str].Some("ada")`).
  Equality is not extended in phase 1.

`match` is the only pattern form; if conditional forms such as `if let` are
added, they bind payloads as `match` does. Should `map`/`and_then` combinators be added later, they follow from the
signature rule: the receiver is the first reference-carrying parameter, so
`o.map(f)` may return a borrow only through `f`'s own origin, its argument.

### `Err` payloads

Recommendation: in `Result`, only the `Ok` payload may carry a reference.
Error values are routinely widened into anonymous sums, printed by `main`, and
propagated far from their origin; a borrowed error would make `?` in an
ordinary `Result[(), IoError | E]` caller fail with an ownership error. Errors
that need to describe input should copy the relevant part, as they do today.

### `&mut` payloads and aliasing

`Option[&mut T]` behaves as an exclusive reference. It is affine: a by-value
match, `?`, `unwrap`, passing it, or returning it moves it, and the extracted
`&mut T` takes over the loan. While the value or its payload binding is live,
the origin is exclusively borrowed, so two `get_mut` results from one
dictionary cannot be live together (dictionary keys already protect the whole
collection). A shared result from an `&mut` origin holds only a shared loan,
as today: `cache.get_ref(&k)` may promote the entry through `&mut self` and still
let the caller read `cache.len()` while the result is live.

Get-or-insert works without new rules, because loans already end per path:

```text
def counter(counts: &mut dict[str, list[i64]], key: &str) -> Result[&mut list[i64], AllocError]:
    match counts.get_mut(key):
        case Some(found):
            return Ok(found)
        case Nothing:
            pass
    counts.insert(*key, []?)?
    Ok(&mut counts[key])
```

### Loans, last use and temporaries

The loan model is the one returned references already use. A
reference-carrying local is a loan holder like a reference local; payload
bindings, `?` results and `unwrap` results are child loans. The origin loan
ends at the last use of all of them on each path. A precise field origin stays
precise; indexed and control-dependent bodies protect the whole argument, as
they do for `&T` results.

Temporaries follow the returned-reference rules unchanged. A method returning
a reference-carrying enum needs a named receiver or a place in named storage.
With a temporary shared argument the result is usable only within its full
expression: `print(lookup(&row(), "id").is_some())?` works; binding the result
is rejected.

### Mutable locals

A `mut` reference-carrying local may be reassigned, which is how a search
remembers its best match without an index:

```text
def longest(words: &list[str]) -> Option[&str]:
    mut best: Option[&str] = Nothing
    for word in words:
        match best:
            case Some(current):
                if word.byte_len() > current.byte_len():
                    best = Some(word)
            case Nothing:
                best = Some(word)
    best
```

It follows [reference rebinding](../design/language/05-ownership/03-reference-rebinding.md):
the first reference it holds fixes its first place, later `Some(...)`
assignments must stay within that place, and assigning `Nothing` keeps the
first place. A binding initialized with `Nothing` takes its first place from its
first `Some` assignment. Recommendation: phase 2, after immutable locals have
shipped, because it touches the rebinding checker.

### Parameters, generics and `Callable`

- **Parameters.** Recommendation: allow reference-carrying enum parameters in
  phase 2. They give optional borrowed arguments
  (`def render(node: &Node, parent: Option[&Node])`) and let helpers take a
  lookup result. Such a parameter counts as a reference parameter when choosing
  the origin, so `def or_first(found: Option[&T], xs: &list[T]) -> &T` may
  return only `found`'s payload. Passing a shared one copies it; passing an
  `&mut` one moves it.
- **Generics.** Type parameters still never bind references, so `Option[T]`
  with `T = &U` stays rejected; a generic function must spell `Option[&T]`.
  Its body is checked per instance, as reference-returning generic functions
  are now. Generic class methods such as `LruCache[K, V].get_ref -> Option[&V]` need
  no copyable bound, so they work for every instantiation regardless of
  issue #515. That issue still matters for copying accessors; this proposal does
  not depend on it.
- **`Callable`.** `Callable[[&K], Option[&V]]` is accepted, with the origin
  fixed by the first reference-carrying parameter, as for `&V` results.

### Layout and code generation

No runtime cost and no new representation. An `Option[&T]` is one 128-bit slot,
the same size as `Option[i64]` and `&T`:

| Value | Low word | High word |
| --- | --- | --- |
| `&T` | slot address | tag shift |
| `Some(r)` | slot address | `(shift << 1) \| 1` |
| `Nothing` | 0 | 0 |
| `Ok(r)` in `Result[&T, E]` | slot address | `shift << 1` |
| `Err(e)` | `e`'s word | `(e.tags << 1) \| 1` |

This is the existing nested-tag scheme: `wrap_tagged` and `tagged_payload`
already restore a payload's high word exactly, so construction is a shift and
an `or`, and a match tests bit 0. The shift needs at most 7 bits, so layout
depth accounting charges a reference payload 7 tag bits within the existing
64-bit tag budget.

A null-address niche (the Rust layout) is rejected: the slot is 128 bits either
way, so it saves nothing, and it would add a special case to every tag read
(`sum_tag`, `Unwrap`, `lower_try`, `widen_sum`). Values are not stored, so no
heap or owner-local storage is involved, and there is no cleanup: a
reference-carrying enum is never `managed`.

### Diagnostics

Each rejection names the value, the place, and the fix:

| Case | Message |
| --- | --- |
| Stored in an aggregate | `Option[&Item]` carries a reference and cannot be stored in a list; match it and store an owned copy or an index |
| Class field or user enum declaration | references cannot be stored in class fields; `Option[&T]` and `Result[&T, E]` may only be returned or held in locals |
| No reference parameter | a function returning `Option[&Item]` needs a reference parameter to borrow from |
| Wrong origin | this `Some` borrows `fallback`, but `Option[&Item]` results borrow the first reference parameter, `items` |
| `&mut` payload from a shared origin | `Option[&mut V]` requires the first reference parameter to be `&mut` |
| Reference in `Err` | `Result` errors cannot carry references; copy the part of the input the error reports |
| `match &o` | `o` already holds a reference; match `o` directly |
| `unwrap_or(&other)` | `unwrap_or` cannot choose between two borrows; use `match` |
| Across `yield`, into a scoped worker, into a closure | `found` holds a borrow of `items` and cannot cross `yield` / be sent to a worker / be captured; match it first and pass the reference |
| Temporary origin | `lookup(...)` borrows a temporary argument; bind the argument to a local first |
| Type argument | reference type arguments are not supported; write `Option[&T]` in the signature (existing message, reused) |

### Teaching

- **[Look up a value that may be absent](../tutorial/04-collections/lookup.md)**
  replaces "Optional borrowed lookup is not implemented" with a short
  `dict.get_ref` section: `get` copies, `get_ref` borrows, and the dictionary
  stays borrowed until the payload's last use. Readers already know element
  borrows from the preceding lessons.
- **[Return a reference to a field](../tutorial/06-data/borrowing-methods.md)**
  gains "Return a reference that may be absent", writing `find` with
  `Option[&Item]` and showing `?` on it. This is the first point where readers
  write their own returning functions with references.
- The reference gains a page after returned references,
  "Optional and fallible references", stating rules 1 to 4, the layout, and the
  rejected cases. [Returned references](../design/language/05-ownership/04-returned-references.md)
  and [borrowing](../design/language/05-ownership/02-borrowing.md) link to it
  instead of saying references never enter enums.

The rule to teach is one sentence: *a lookup can return `Option[&T]`; treat
the result like the reference it holds, and never store it.*

## Standard library changes

Naming follows one rule across builtin and std containers (decided after this
proposal was accepted; see
[copying and borrowed lookup](../design/facilities/01-builtins/collections.md#copying-and-borrowed-lookup)):
indexing and plain accessors trap; `get` and `peek` return a copy in an
`Option`; `get_ref`, `peek_ref`, and `get_mut`/`peek_mut` return borrows.

| Module | Today | Proposed |
| --- | --- | --- |
| builtin `dict` | `get(k) -> Option[V]` copies non-affine values | Add `get_ref(k) -> Option[&V]` and `get_mut(k) -> Option[&mut V]` for every value type |
| builtin `list` | `get(i) -> Option[T]` copies | Add `get_ref(i)` and `get_mut(i)`; `get_ref(0)` and `get_ref(-1)` replace a separate `first`/`last` |
| `std.cache.lru` | `get_with`, `peek_with`, trapping `get_ref`, `peek_mut` | Copying `get(&mut self, k) -> Option[V]` (promotes) and `peek(k) -> Option[V]`, possible now that methods are checked only when used (issue #515); `get_ref -> Option[&V]` (promotes), `get_mut -> Option[&mut V]`, `peek_ref -> Option[&V]`, `peek_mut -> Option[&mut V]`; remove the callback and trapping forms (issue #516) |
| `std.graph` | `node`/`edge` trap; callers test `contains_node` first | Keep trapping `node`/`edge` (handle indexing); add `get_node_ref`, `get_node_mut`, `get_edge_ref`, `get_edge_mut` |
| `std.collections` | `Heap.peek`, `HeapBy.peek` trap when empty | `peek_ref() -> Option[&T]`, and a copying `peek() -> Option[T]` for copyable `T`, matching `pop() -> Option[T]` |
| `std.deque` | No access without removal | Add `get_ref(i)`/`get_mut(i)` if a use case appears |
| `std.http`, `std.http.cookie`, `std.http.body`, `std.json`, `std.toml` | `find(...) -> Option[i64]` | `find(...) -> Option[&Header]` (and the matching element type); callers currently index immediately |
| pipelines | `next`, `find`, `reduce` reject `&T` items | Phase 3: return `Option[&T]` borrowing the pipeline's source |

Not affected: `regex` `Captures.get` (copyable `Match`), `env.get` (owned
text), and `toml.Document.get`, whose arena nodes are not `Toml` values and
which therefore still builds a copy.

Std has no stability promise yet, so the replaced forms are removed in the same
change rather than deprecated, keeping one obvious way.

## Alternatives considered

| Alternative | Why rejected |
| --- | --- |
| Status quo with callbacks | Every container repeats the work; callbacks cannot `?`, `return` or `break`; signatures gain `R` and `F` parameters |
| Membership test plus trapping getter | Two lookups per access; a forgotten test is a crash rather than a type error |
| Handles and indices | A second bounds check, no relation between index and collection, and impossible for private layouts and dictionaries |
| A dedicated optional-reference type (`&T?` or `Ref[T]`) | New syntax and a type learners must map back to `Option`; `match` and `?` already handle `Option` |
| Any enum carrying references | Needs reference fields in declarations or reference type arguments; see [Which enums qualify](#which-enums-qualify) |
| References in all aggregates with lifetime parameters | Breaks the no-annotation design goal and adds checker and learner cost far beyond these use cases. This proposal does not preclude a later B11 decision on stored references |
| Returning `Option[&T]` but forbidding local bindings | Forces every lookup into a `match` scrutinee; `?` could not bind its result |

## Implementation plan

1. **Types and checking.** Add a `carries_loan` predicate (references and
   reference-carrying enums) and use it where origin selection,
   `return_origin`, `check_return_reference`, trailing-expression checks and
   `call_reference_result` now test `Ty::Ref`. Admit reference payloads in
   `Option`/`Result` resolution only for return types and local annotations;
   keep the storage barrier for every other position, with the messages above.
   Make `?`, `unwrap` and `ok_or` forward the operand's loan, and by-value
   matches create child payload loans. Charge 7 tag bits in layout depth.
   Tests: acceptance, every rejection row, loan ends per arm, `&mut` aliasing,
   temporaries, generic per-instance checking, and codegen of nested
   `Result[Option[&T], E]`.
2. **Builtins and docs.** `dict.get_ref`/`get_mut`, `list.get_ref`/`get_mut`;
   the new reference page; the two tutorial sections.
3. **Std adoption.** `std.cache.lru` (issue #516), `std.graph`, `Heap.peek`,
   the `find` functions, each with its reference page and tests.
4. **Extensions.** `mut` reference-carrying locals, reference-carrying
   parameters, then pipeline `next`/`find`/`reduce` over `&T` items.

Each step is usable alone; step 1 with `Option` alone would already serve user
code.

## Open questions

1. **Scope: `Option` only, or `Option` and `Result`?** Recommendation:
   both, with nesting, because fallible borrowed lookups are common and the two
   share one implementation.
2. **May `Err` carry a reference?** Recommendation: no; see
   [`Err` payloads](#err-payloads).
3. **Nesting `Result[Option[&T], E]`, including `Ok(found)` at a return.**
   Recommendation: allow. It is still one value produced by one call, and
   rule 4 continues to reject every stored position.
4. **Local construction (`found = Some(&xs[0])`).** Recommendation: allow in
   phase 1; it is `r = &xs[0]` plus a tag. `mut` reassignment waits for phase 2.
5. **Reference-carrying parameters.** Recommendation: allow in phase 2, with
   the parameter counting toward origin selection.
6. **Builtin names: `get_ref`/`get_mut`, or change `get` to borrow affine
   values?** Recommendation: new names. One method whose result is a copy for
   `i64` and a borrow for `list[i64]` would hide which one the reader has.
   **Decided:** `get`/`peek` copy and `get_ref`/`peek_ref`/`*_mut` borrow, in
   every container, including `std.cache.lru`.
7. **Trapping accessors: replace or keep beside `get_*`?** Recommendation:
   change `Heap.peek` to return `Option`; keep `graph.node`/`edge` trapping
   like indexing and add `get_node_ref`/`get_edge_ref`; remove the LRU callback and
   trapping getters.
8. **`unwrap_or` with a reference default.** Recommendation: reject, as
   conditional reference expressions are.
