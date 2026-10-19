# User-defined ordering

**Status: Not implemented.** Classes and enums have structural `==` and no
ordering. The current contracts are [classes](../design/language/03-types/classes.md),
[enums](../design/language/03-types/enums.md), [tuple ordering](../design/language/03-types/tuples.md),
[list sorting](../design/facilities/01-builtins/lists.md) and
[`std.collections`](../design/stdlib/collections.md). Remaining work belongs in
[B37](../backlog.md). Equality is shared with the sibling
[hashing proposal](user-type-hashing.md).

## Problem

Ordering accepts integers, floats, `str`, and tuples of those. Every other type
is rejected: `a < b` on a class reports "operator `<` does not accept", and
`list.sort()`, `sorted`, `Heap[T]`, and the `std.math` helpers `min` and `max`
inherit that limit. The workarounds are per call site:

- `items.sort_by_key(key)` with a key function returning an orderable tuple;
- `HeapBy(before)` with a stored comparator;
- an ad hoc method such as `as_nanoseconds()` in
  [`std.time`](../design/stdlib/time.md), which says "Ordering has no operator".

A key cannot be a reference, so a key holding a `str` field copies the string
on every comparison. A type that should have one natural order states it again
at every sort, heap, and comparison, and the statements can disagree. Classes
are not orderable tuple components, so `(priority, task)` cannot be ordered
when `task` is a class.

Only `__init__`, `__del__`, `__enter__`, and `__exit__` are recognized hooks
today; any other dunder is an ordinary method.

## Recommendation

A class or enum defines its order with one three-way method:

```text
class Version:
    major: i64
    minor: i64
    label: str

    def __cmp__(self, other: &Version) -> Order:
        order = self.major.compare(&other.major)
        if order != Order.Equal:
            return order
        order = self.minor.compare(&other.minor)
        if order != Order.Equal:
            return order
        self.label.compare(&other.label)
```

`Order` is a prelude enum with nullary variants `Less`, `Equal`, and `Greater`.
Every orderable built-in value gains `x.compare(&y) -> Order`, using the
existing total sorting order. A type with `__cmp__` is orderable everywhere an
orderable built-in is: all six comparison operators, `sort`, `sorted`, keys of
`sort_by_key`, `Heap[T]`, tuple components, and generic bodies that use `<`.

### Rules

1. The signature is exactly `def __cmp__(self, other: &T) -> Order`, where `T`
   is the declaring type (for a generic type, the same instance). It has no type
   parameters, defaults, or `Result`. Classes and enums may define it; anonymous
   sums may not.
2. All six operators derive from it. `a < b` is `__cmp__(&a, &b) == Less`;
   `a <= b` is `!= Greater`; `a > b` and `a >= b` are the mirrored tests;
   `a == b` is `== Equal`; `a != b` is `!= Equal`. Individual operators cannot
   be defined.
3. Defining `__cmp__` replaces structural `==` and `!=` for that type. There is
   one source of truth for equality and order, and they cannot disagree.
4. `__cmp__` is not called by name. `a.compare(&b)` is the spelling, for user
   and built-in types alike. This follows the existing rule that rejects
   explicit `__init__`/`__del__` calls.
5. Like `__del__`, the hook belongs to the type: it applies wherever the type
   is visible, independent of the method's own visibility.
6. The order must be total and consistent: antisymmetric, transitive, and
   stable for unchanged values. The compiler cannot check this. A violation
   gives an unspecified order, as an inconsistent `HeapBy` comparator does
   today, but cannot break memory safety.
7. Its body may perform only computation, as the
   [worker effect](../design/facilities/03-concurrency/effects.md) walk
   classifies it: no I/O, clock, entropy, or process effects.

## Alternatives compared

| | (a) six dunders | (b) one `__cmp__` | (c) derived | (d) comparators only |
| --- | --- | --- | --- | --- |
| Methods written | Up to six, or a `total_ordering` rule | One | None | One per call site |
| Operators can disagree | Yes | No | No | Not applicable |
| `==` consistent with order | By convention | By construction | By construction | No relation |
| Lexicographic composition | Two calls per field (`a < b`, `b < a`) | One call per field | Generated | Key tuples |
| Field order is semantic | No | No | Yes | No |
| New surface | Six hook names | One hook, `Order`, `compare` | Opt-in syntax or none | None |

**(a) Python's six rich comparisons** (`__eq__`, `__ne__`, `__lt__`, `__le__`,
`__gt__`, `__ge__`). Python lets each disagree and returns `NotImplemented` for
mixed types; `functools.total_ordering` fills in the missing ones. A Plenty
version would need a rule for which subsets are legal, a consistency rule it
cannot check, and checker work for six signatures per type. Sorting needs only
`<`, but composing an order over fields needs equality as well, so a tuple
`(priority, task)` would call `__lt__` twice per tied component. (CPython's C
API combines these into one `tp_richcompare` slot with an operator argument,
which PyO3 exposes as `__richcmp__`; that is a dispatch detail, not a
Python-level method.)

**(b) One three-way method**, as in Rust's `Ord::cmp`, Python 2's `__cmp__`,
and C++'s `operator<=>`. One method answers every operator and composes
lexicographically in one call per component. The cost of deriving an operator
is one compare of the returned tag (see [Costs](#costs)).

**(c) Derived structural ordering**, lexicographic over fields in declaration
order like tuples. Automatic ordering would make every class orderable,
including types where order is meaningless (instants from different clocks,
handles), and would make reordering fields a silent behavior change. Opt-in
derivation needs attribute or decorator syntax that Plenty does not have, and
[B45](../backlog.md) already declines automatic derive for formatting.
Defer it; a later derive, if any, generates an ordinary `__cmp__`.

**(d) Comparators only** (status quo). It keeps the language unchanged, but
leaves `<`, `min`, `max`, `Heap[T]`, and tuple components unavailable, repeats
the order at each use, and makes `str` keys copy per comparison. Keep
`sort_by_key` and `HeapBy` for orders other than the natural one.

## Total and partial order

Plenty has one total order for sorting and tuple operators, and IEEE behavior
for scalar float operators. Rust splits `PartialOrd` from `Ord` so that floats
can have operators but not `sort`; learners then meet `total_cmp` anyway.
Plenty should not add that split. `__cmp__` returns one of three answers, so
every user order is total. `x.compare(&y)` on floats uses the sorting order:
`-0.0` before `0.0`, every NaN equal to every other NaN and after all numbers.
A class wrapping an `f64` and comparing with `compare` therefore has
`NaN == NaN` under its own `==`, matching its order. That differs from scalar
`f64 ==`, deliberately: the class's equality follows its declared order.

There is no `Unordered` variant. A type with values that genuinely do not
compare (instants from different clocks) should not define `__cmp__`.

## Equality

Classes and enums currently use structural equality, which the
[hashing proposal](user-type-hashing.md) also depends on: a hash must agree
with `==`. Rule 3 makes `__cmp__` the definition of equality for an ordered
type. The hashing proposal admits only structural equality and hashing, so a
type that defines `__cmp__` is not a key type: its `==` is user code that a
structural hash cannot follow. To key a dictionary by such a value, key on a
projection of the fields `__cmp__` compares. A type without `__cmp__` keeps
structural `==` unchanged and stays eligible as a key.

Making `__cmp__` define `==` also lifts a limit: structural equality rejects
recursive data, but a written `__cmp__` may recurse explicitly through
`Box[T]` fields, giving such types both equality and order.

## Use sites

**Operators and generic code.** Generic bodies are checked per concrete
instance ([protocols](../design/language/07-generics/protocols.md)), so
`std.math.min`, `max`, `std.test.less`, and `Heap[T]` accept ordered user types
without signature changes. `min_all`/`max_all` still require copyable elements,
and class instances are affine, so they remain unavailable for classes.

**Protocols.** No `Ordered` protocol is needed for generic bodies. Protocols
constrain only classes, enums, and `bytes`, so a protocol could not admit `i64`
today. A user protocol may still require `__cmp__` to restrict a parameter to
ordered user types; whether built-in orderable types should satisfy such a
protocol is an open question.

**Sorting.** `items.sort()` and `sorted(items)` accept element types with
`__cmp__`; `sort_by_key` accepts keys of such types. The sort itself allocates
nothing; allocation inside `__cmp__` is the type's own, as for key callbacks.

**Tuples.** A tuple whose components are orderable, including ordered user
types, is orderable. `Heap[(i64, Task)]` orders by priority, then by `Task`.

**Dictionaries and sets.** Unchanged here; see the
[hashing proposal](user-type-hashing.md).

## Ownership

Both operands are shared borrows: `self` and `other: &T`. A comparison never
moves, copies, or drops an operand. `a < b` on owned values borrows both for
the call, as tuple comparison observes them today; comparing `&T` values passes
the references through. Because the receiver is shared, `__cmp__` cannot
mutate either value, so a sort cannot change the elements it is ordering. The
list remains exclusively borrowed by `sort`; the hook receives element loans
derived from that borrow, as `sort_by_key` callbacks do.

## Errors, allocation, and workers

`__cmp__` cannot fail: it returns `Order`, not `Result`, so `?` is unavailable
inside it. Allocation in Plenty is always fallible and visible as a `Result`,
so a comparison that allocates must handle the failure locally. Traps
(overflow, indexing) remain traps and abort the sort like any other trap.

Rule 7 restricts the body to computation. This keeps comparisons pure for
learners, lets `sort` keep its documented contract apart from the hook's own
allocation, and makes every ordered type usable from workers. It reuses the
existing closed-world walk in `src/effects.rs` and `threading::check`, which
already certifies every reachable body; no new eligibility mechanism is needed.
Whether to also forbid allocation is an open question.

## Costs

### Measured today

A probe on this branch (`--release`, Cranelift 0.131, 1,000,000 pairs, single
core, median of seven runs, construction subtracted; checksums equal):

| Sort | Path | Time |
| --- | --- | ---: |
| `list[i64].sort()` | Runtime, Rust `sort_unstable_by` over scalar rows | 78 ms |
| `list[(i64, i64)].sort()` | Runtime heapsort, descriptor-interpreted compare | 1,444 ms |
| `list[Point].sort_by_key(key)` | Plenty-source heapsort, runtime `OrderCompare` per comparison | 4,142 ms |
| `list[Point]` heapsort in Plenty, direct `cmp(&Point, &Point) -> i64` | Models the hook lowered through a generated sort | 2,473 ms |

`perf` attributes the last row's time to runtime calls for element access and
row exchange (`plenty_collection` 48%, `Payload::relocate` 15%,
`plenty_retain`/`plenty_release` 19%, `collection_at` 7%). The comparison
function itself is 4%; replacing the call with inline field compares did not
improve the time. The tuple sort spends 27% in the runtime's interpretive
`compare` (`plenty-runtime/src/aggregates.rs`), which dispatches on the
descriptor kind and recurses per field on every comparison. Sizes: the direct
comparison function is 400 bytes, a specialized Plenty heapsort 2.8 KB, and a
`sort_by_key` instantiation 3.7 KB.

The comparison call is not the bottleneck; how the sort reaches the elements
is.

### Dispatch

The hook is resolved statically: `a < b` lowers to a direct call of the
class's mangled method, as `with` lowers to `Op::Call` of `__enter__` in
`src/frontend/contexts.rs`. There is no vtable or runtime lookup. Plenty does
not inline calls. Cranelift 0.131 offers `Context::inline` with an
embedder-supplied `Inline` policy, but the compiler does not use it, so every
user comparison costs one call. A probe returning a three-variant nullary enum
compiled to a register return with no runtime call; a `less` wrapper around it
was 39 bytes. Deriving `<` from `__cmp__` therefore costs one integer compare
on the returned tag, versus a direct `__lt__`: no measurable difference, and
lexicographic composition saves a second call per tied component.

Compared with built-ins: integer `<` is one instruction; `str` and tuple `<`
already call the runtime (`TextCompare`, `OrderCompare` through
`plenty_collection`) and pay its dispatch. A user `__cmp__` over integer
fields is a direct call with field loads, and is expected to be no slower than
tuple `<`; measure it (below).

### Sort path

Two lowerings are possible for `list.sort()` on user elements:

1. **Runtime sort with a descriptor hook.** Runtime type descriptors already
   carry one callback into generated code: the class `__del__` adapter
   (`Type::drop` in `plenty-runtime/src/aggregates.rs`, emitted by
   `src/codegen/metadata.rs`). A `compare` slot of the same kind lets the
   runtime's `compare` call `__cmp__` for a record that has one. This makes
   `sort`, tuple operators with user components, and `OrderCompare` keys work
   with one runtime change, keeps the runtime's row exchange, and adds one
   indirect call per comparison. It grows every descriptor from 64 to 72
   bytes of static data, unless the slot replaces padding in a reorganized
   layout.
2. **Generated sort**, specialized per element type like `sort_by_key` in
   `src/frontend/sorting.plenty`. It calls the hook directly, but the probe
   shows such a sort is 1.7 times slower than the runtime tuple sort because
   each step reaches elements through runtime calls, and it adds about 3 KB
   per element type.

Recommend option 1 for `sort`, `sorted`, tuple components, and keys, and
direct generated calls for operators in ordinary and generic code.

### Compile time

Detection is one method-name lookup per class or enum and one signature check,
done once per type; operator typing gains one branch in `is_orderable`. Each
instantiation of a generic body using `<` on a new type is already a separate
specialization, so ordered user types add no new instantiation mechanism. The
probe programs checked in about 2 ms each, too small to separate the cost.

### Measurements required before shipping

- Extend `examples/collection_bench.py` with sorts of `list[(i64, i64)]`,
  `list[Point]` by `__cmp__` (both lowerings above), `list[Point]` by
  `sort_by_key`, and a `str`-field class, at 100,000 and 1,000,000 elements,
  recording binary size per variant.
- A `Heap[Task]` push/pop benchmark against `HeapBy` with the same order.
- `compile_bench` `--workload check` and `--workload aot` (see
  [profiling](../development/profiling.md)) before and after, with a generated
  workload of many ordered classes compared and sorted, to confirm detection
  and specialization cost stays in the noise.
- Object-size check of the descriptor change across the test suite's
  executables.

## Naming

`std.atomic` already exports `Ordering` for memory ordering. It is not in the
prelude, but `from std.atomic import Ordering` would collide with a prelude
`Ordering`. Use `Order` for comparisons: short, distinct, and read naturally
as `Order.Less`. The hook name `__cmp__` follows Python 2 and matches the
three-way shape; `__order__` is the alternative if the Python 2 association is
unwanted.

`Order` provides `reverse(self) -> Order` for descending orders and may provide
`then(self, next: Order) -> Order` for short compositions. `then` evaluates its
argument eagerly, so early returns stay the idiom for expensive fields.

## Rejected

- **Six rich-comparison dunders**: allows inconsistent operators and doubles
  calls in lexicographic composition.
- **`PartialOrd`/`Ord` split**: two concepts for learners; Plenty already chose
  one total order for sorting.
- **Automatic structural ordering**: makes field order semantic and gives order
  to types where it is meaningless.
- **Keeping structural `==` beside `__cmp__`**: two definitions of equality
  that can disagree, breaking sorting and hashing invariants.
- **Fallible comparisons** (`-> Result[Order, E]`): every operator and sort
  would become fallible; a failing comparison has no sound place in `<`.
- **A runtime callback for every comparison in generic code**: the static call
  is cheaper and already available.

## Open questions

- Should `__cmp__` also be forbidden from allocating, beyond rule 7?
- Should built-in orderable types satisfy a protocol that requires `__cmp__`?
  That needs methods on `i64` and tuples visible to protocol matching.
- Should `Order` and `compare` be prelude names, or imported from a small
  module? The hook's signature mentions `Order`, which argues for the prelude.
- Does `then` earn its place, or should composition use early returns only?
- Should nullary enums get a declared-order shortcut, or keep writing
  `__cmp__` over a rank method?
- Coordination with the [hashing proposal](user-type-hashing.md): one
  equality hook, and the required agreement between hash and `__cmp__`.

## Staged implementation

1. **Hook and operators.** Recognize `__cmp__` beside `__init__`/`__del__`
   (`src/frontend/classes.rs`, `src/frontend/enums.rs`), validate its
   signature, and reject direct calls. Add `Order` to the prelude
   (`src/frontend/enums.rs`, `src/frontend.rs`) and `compare` for built-in
   orderable types. Extend `Ty::is_orderable` (`src/op.rs`) and binary operator
   typing (`src/frontend.rs`, near "operator `{op}` does not accept") so
   comparisons on such types lower to the hook call and a tag test; route
   `==`/`!=` there too instead of structural equality (`lower_equality` in
   `src/codegen.rs`). Enforce rule 7 through `src/effects.rs`. Generic users
   (`std.math`, `std.test`, `Heap[T]`) then work unchanged.
2. **Runtime ordering.** Add the descriptor `compare` slot
   (`plenty-runtime/src/aggregates.rs`, `src/codegen/metadata.rs`) with an
   adapter like the `__del__` hook, so `sort`, `sorted`, `OrderCompare` keys,
   and tuple components with user types use it
   (`src/frontend/sorting.rs`, `int_cmp` in `src/codegen.rs`).
3. **Documentation and lessons.** Update the class, enum, tuple, list, and
   `std.collections` reference pages, `std.time` (which can then define
   `Duration` ordering), the priority-queue lesson, and B37.

Each stage ships with diagnostics tests for invalid signatures, direct calls,
effectful bodies, and inconsistent orders that stay memory safe, plus the
measurements above.
