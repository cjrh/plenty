# Collections and iteration

`[elements]`, `{elements}`, and `{key: value}` construct a collection
with recoverable output allocation, returning `Result[collection, AllocError]`.
An expected Result type supplies empty-display element types. The owner is
allocated first, then entries evaluate left to right; the first failed insertion
stops construction and releases the initialized prefix. Unevaluated entries have
no effects. Nested expressions return their own results;
use nested postfix `?` operations to extract their values and propagate failure. `?` still propagates
from the enclosing function, not into a surrounding display's Result. This is
an explicit construction expression, not exception handling.

Comprehensions also return Result, including nested loops and filters.
After output growth fails, no iterator advances or later filter/element expression
runs. Active hidden iterators and yielded owners are cleaned up. Iterable
construction and expressions inside the comprehension retain their own contracts.

`list[T].from(source)` and `set[T].from(source)` consume an owned
collection, range, or generator and return `Result[list[T], AllocError]` or
`Result[set[T], AllocError]`. Output construction and growth are fallible.
Sets require hashable elements and preserve the first occurrence of each value.
Failure destroys the partial output, current element, and remaining source;
generator side effects before failure are not rolled back. Source construction
and allocations performed by a generator body keep their own failure contracts.
Borrowed sources and string iteration are not accepted by this API yet.

The compiler-known constructors `list[T]`, `set[T]`, and `dict[K, V]` accept
concrete element types, including nested collections. Dictionary keys and set
elements are restricted to integers, `bool`, and `str`; there is no user-defined
hash/equality protocol. Unit elements are rejected. These built-ins use concrete
compiler-known operations and require no trait solver.

Literals use Python spelling: `[1, 2]`, `{"a": 1}`, and `{1, 2}`. Elements must
have exactly the same type. Empty literals need context from an annotation,
parameter, or return type; typed constructors such as `list[i64]()`,
`dict[str, i64]()`, and `set[i64]()` also work. `{}` always means dictionary.
Nonempty literals infer their type from the first element; integer literals still
default to i64 without context; collection annotations guide literal entries.
Already typed entries never implicitly narrow. Aliases may name collection types.

Assignment, owned arguments, and returns move collections. Independent duplication
requires `copy(value)`, which recursively copies mutable contents while retaining
immutable strings and enums. `append`, `add`, and indexed updates mutate in place
through a named `mut` owner or an exclusive reference. Mutation arguments and
indices are evaluated before exclusive access to the target is taken, supporting
`xs.append(len(xs))` and `xs[len(xs) - 1] = value` without two-phase loans.
Nested indexed places support explicit element references and mutation through
those references. Element loans conservatively protect the whole collection.
Owned values cannot be moved directly out of indexed storage.

Lists preserve order and duplicates. Dictionaries preserve first insertion order;
a repeated key replaces its value without moving the key. Sets remove duplicates
and promise no iteration order. Equality is structural: list order matters;
dictionary insertion order and set order do not. There are no identity tests.

`len`, `in`, and `not in` work with collections, strings, and ranges.
Generators support consuming iteration, not length or membership. Lists, strings, and
ranges support i64 indexing, including negative indices. Dictionaries index by
their key type. Out-of-bounds indices and absent keys report a runtime error and
exit with status 1. `dictionary.get(key)` returns `Option[V]`: `Some(value)`
for a present key and `Nothing` for a missing key. It takes exactly one key,
with no default argument. The receiver and key are observed, may be references,
and are evaluated once in source order. Lookup does not allocate: scalar values
are copied and immutable managed values are retained. The returned value survives
replacement of the entry or destruction of the dictionary. Constructing either
input expression still uses that expression's allocation policy.

`get` supports only non-affine values, like ordinary indexed reads: numbers,
booleans, strings, and enums whose payloads do not transfer ownership. Collections,
classes, and enums containing them are rejected, even for temporary dictionaries;
there is no hidden deep copy or alias to mutable storage. Use `pop` to remove and
take ownership of such values; explicit indexed references borrow them instead.
Optional borrowed access awaits stored-reference support. The `Option` wrapper keeps
stored absence distinct from a missing key: a stored `Nothing` is returned as
`Some(Nothing)`.

`items.get(index)` provides the same optional read for lists, using an `i64`
index and returning `Option[T]`. Negative indices count from the end; empty lists
and out-of-range indices return `Nothing`, including extreme `i64` values. The
receiver and index are observed once in source order, may be references, and remain
usable. Lookup is constant-time and does not allocate or change the list. Managed
immutable results retain their existing storage, surviving entry replacement or
list destruction. As with dictionary `get`, affine elements are rejected: use
`pop` to remove and take ownership, or explicit copying with ordinary indexing.
There is no default argument. Unlike string `get`, list lookup does not create
new character storage and therefore needs no allocation-error result. Constructing
the receiver or index expression still follows its own allocation policy.

`dictionary.pop(key)` requires a mutable binding, mutable class field, or exclusive
reference and returns `Option[V]`. A hit removes the entry and transfers its value
into `Some`; a miss returns `Nothing` without changing the dictionary. All permitted
dictionary value types work, including lists, classes with custom cleanup, and
enums containing owned payloads. The removed value is never copied or destroyed by
removal; its returned owner is responsible for cleanup. Discarding that result
performs ordinary automatic cleanup. The removed key's stored owner is released.

`pop` takes exactly one key, with no default argument. Its key expression is
observed once before taking the receiver's exclusive loan, as with other collection
mutations. Key references are accepted, and an immutable key derived from the same
dictionary can be used. Active conflicting loans still prevent mutation. Temporary
dictionary receivers and set `pop` are not supported; sets use `discard`.

Removal itself does not allocate, and both buffers retain their capacity. Remaining
entries keep their insertion order; reinserting a removed key appends it at the end.
The initial implementation shifts entries and rebuilds buckets in existing storage:
successful removal scans the reserved table and rehashes the remaining keys,
while a missing key uses the normal hash lookup. Key evaluation and subsequent user cleanup retain
their own allocation policies. This is not a promise of constant-time removal.

`items.pop()` removes a list's last element; `items.pop(index)` selects an `i64`
index, including negative indices counted from the end. Both return `Option[T]`
and transfer the selected element's owner into `Some`. Empty lists and out-of-range
indices return `Nothing` without changing the list, including extreme `i64` inputs.
All supported list element types work, including nested collections and classes
with custom cleanup. The remaining elements preserve their order and the list
keeps its capacity; removal allocates nothing. Removing the last element is constant
time, while earlier removal shifts the remaining elements.

List `pop` requires a mutable binding, mutable field, or exclusive reference.
An explicit index (or reference to one) is evaluated once before borrowing the
receiver exclusively, so `items.pop(len(items) - 1)` is valid. A conflicting loan
that remains active afterward still prevents removal. Temporary receivers and
more than one index argument are rejected. Removed owners clean up normally,
including when the returned `Option` is discarded or a later `?` propagates.

`values.discard(value)` removes a set member and returns `True` if present,
otherwise `False`. A miss does not change the set. It requires a mutable binding,
mutable field, or exclusive reference and exactly one argument of the set's element
type (or a reference to it). The argument is observed once before taking the
receiver's exclusive loan and remains available afterward. Active conflicting
loans prevent removal; temporary receivers are not supported. User-defined class
methods named `discard` continue to use ordinary method dispatch.

Set removal releases the stored member's owner, reuses both buffers, and allocates
nothing. It shares dictionary hash-index rebuilding: a hit scans the reserved table
and rehashes remaining keys; a miss uses normal lookup. Reinsertion can reuse the
vacated capacity. Set iteration order remains unspecified. The Boolean result
distinguishes removal from absence, including for zero, `False`, and empty strings;
there is no exception or allocation-error result. Argument construction retains its
own allocation policy.

`list(iterable)` and `set(iterable)` convert supported iterables; `dict(d)`
transfers an existing dictionary value. Dictionary `keys()` and `values()` produce
new lists. `values()` on mutable payloads requires an owned temporary such as
`copy(d).values()` so a borrowed dictionary cannot expose mutable aliases.
`items()` is a borrowed loop/comprehension view, described below. Pair-iterable
dictionary construction remains deferred.

`dictionary.keys()` and `dictionary.values()` are fallible snapshot
operations returning `Result[list[K], AllocError]` and
`Result[list[V], AllocError]`. Both take no arguments, evaluate the receiver once,
and preserve insertion order. They allocate a new list, retaining existing
immutable element storage rather than deep-copying it. For non-affine values,
the receiver is observed and can be a binding, field, or reference; the snapshot
survives source updates and destruction. `keys()` also observes dictionaries
with affine values, without touching those values.

Like `values()`, `values()` with affine payloads requires an owned temporary,
whose values transfer to the result. A named binding or reference cannot expose
mutable aliases through this operation. Use `copy(d)?.values()` to
explicitly request fallible duplication while preserving `d`, or call it on a
function result to transfer its owned payloads, including custom-cleanup classes.
The temporary is consumed on both success and failure; failed allocation cleans
its original payloads normally. Borrowed receivers remain unchanged on failure.

The runtime reserves the entire entry buffer and list header before retaining or
transferring elements. Nonempty snapshots require two allocations; empty snapshots
require only the header. After reservation, copying the slots cannot allocate or
invoke user code. Capacity/layout overflow returns `AllocError.CapacityOverflow`;
allocation failure returns `AllocError.OutOfMemory`. The `Result` wrapper needs
no allocation. Receiver construction and user cleanup retain their own allocation
policies. Ordinary `keys()` and `values()` still terminate on allocation failure.

`items.reverse() -> ()` reverses a list in place through a named mutable owner,
class field, or exclusive reference. It takes no arguments and allocates nothing.
Element slots are reordered without cloning, retaining, or dropping payloads;
all permitted list element types are supported. Later list destruction follows
the new element order. Empty and one-element lists are unchanged. Temporary
receivers and shared references are rejected; the usual overlapping-loan rules
apply. This does not introduce a reversed iterator or reverse slice steps.

`collection.clear() -> ()` empties a list, dictionary, or set through a mutable
binding, class field, or exclusive reference. It takes no arguments. It retains
the collection's buffers and capacity, resets any hash index, and allocates no
runtime storage. Removed owners are dropped before returning: lists in element
order and dictionaries in insertion order with keys before values. Set order is
unspecified. User cleanup retains its own allocation/effect policy. Empty clear
is harmless, future insertions can reuse capacity, and independent immutable
owners survive. Temporaries, shared references, and conflicting loans are rejected.

`items.extend(other) -> Result[(), AllocError]` appends all elements of a
same-typed owned list. The destination requires a named mutable binding, class
field, or exclusive reference. The source is evaluated and moved before exclusive
access to the destination, as with `append`. It is consumed on both outcomes:
on success its elements transfer in order without cloning; on failure its owners
are dropped. Destination contents stay unchanged on failure. Preserve a source
explicitly with `items.extend(copy(source)?)`; references and arbitrary
iterables are not accepted as sources. Self-extension by moving the destination
is rejected by ownership checking.

Reserve the entire additional entry count before moving anything. When capacity
suffices, including empty sources, the operation allocates nothing; otherwise
reservation returns `OutOfMemory` or `CapacityOverflow`. After reservation only
slot transfer remains. All permitted list element types, including custom-cleanup
classes, work. Input construction and user cleanup retain their own policies.

`dictionary.update(other) -> Result[(), AllocError]` consumes a same-typed
dictionary, with the same receiver and source ownership rules as `extend`.
Existing keys retain their insertion positions and stored key owners, while
incoming values replace their payloads. New keys append in source insertion
order. Count keys absent from the destination and reserve all entry/hash storage
before any replacement, insertion, or payload destruction. A reservation failure
leaves destination contents and lookup behavior unchanged; the consumed source
is then cleaned normally. Capacity may change during preparation.

Once reservation succeeds, transfer source entries without copying. Replacement
drops old destination payloads in source traversal order, after installing each
new payload. Incoming duplicate-key owners are released; new-key owners transfer.
Replacing only existing keys, empty sources, and updates that fit reserved storage
allocate nothing in the runtime. User cleanup keeps its own effect/allocation
policy. Use `copy(source)?` to preserve an input; source references, pair
iterables, and keyword updates are deferred.

`set.update(other) -> Result[(), AllocError]` consumes a same-typed set and
adds its missing members. The destination requires exclusive access; the source
is consumed even on failure. Count absent members and reserve entries/buckets
before changing contents. Allocation failure leaves destination membership and
lookup behavior unchanged. On success, transfer new member owners and release
duplicate incoming owners while keeping the destination's existing owners.
Empty and duplicate-only sources need no allocation; sufficient reserved capacity
also avoids allocation. Set iteration order stays unspecified. Use `copy`
explicitly to preserve the source; general iterables, source references, and
multi-source update calls are deferred. Hashing/comparison of permitted member
types invokes no user code or allocation.

`set.union(other) -> Result[set[T], AllocError]` observes both same-typed sets
and returns their unique members in an independent set. Inputs can be references
or the same set. All output storage is reserved before retaining member owners;
allocation failure leaves both inputs unchanged. Immutable strings may share
storage. Nonempty results use three allocations (buckets, entries, owner header),
empty results only the header. Iteration order is unspecified.

`set.intersection(other)` has the same result and borrowing contract as
`union`, selecting only common members. It reserves for the actual result
size, so disjoint inputs need only an empty output owner header.

`set.difference(other)` selects receiver members absent from `other`, using
the same fallible, borrowed, exact-capacity contract. Unlike union and intersection,
the operands are not interchangeable; subtracting a set from itself yields empty.

`set.symmetric_difference(other)` selects members present in exactly one
input. It shares the same fallible construction contract; equal inputs produce
an empty result. Set algebra methods accept exactly one same-typed set, not
arbitrary iterables, and do not imply overloaded arithmetic operators.

`set.intersection_update(other) -> ()` retains common members in the receiver,
requiring a mutable binding, field, or exclusive reference. The same-typed source
is observed before acquiring the destination's exclusive loan and remains borrowed
through mutation. Self-aliasing is rejected. Removed immutable member owners are
released; entry and hash capacity are retained and the index is rebuilt without
allocation. Both lookups and future insertion remain valid.

`set.difference_update(other) -> ()` follows the same borrowing and no-allocation
contract, removing every member also present in `other`. An empty source changes
nothing. It retains capacity even when all members are removed. Neither method
accepts source aliases into the destination; use `clear()` to empty a set directly.

Set relationship methods take one same-typed set, including shared references,
and return `bool`. They observe both operands once, left to right, without moving
them or allocating. Empty sets are subsets of all sets and disjoint from all sets.
No iterable conversion, comparison operators, or heterogeneous key coercion is implied.

`items.count(value) -> i64` counts equal elements; `find(value)` and `rfind(value)`
return the first or last matching zero-based index as `Option[i64]`. Missing
values produce `Nothing`, not a sentinel or exception. These methods currently
support integer, float, bool, and string elements only, guaranteeing allocation-free
comparison. Aggregate search remains outside this initial method subset. Floats use IEEE equality (NaN
never matches; signed zeros compare equal). Both operands are observed once in
source order, including references, and must have exactly matching element types.
Optional bounds and Python's throwing `index` method are deferred.

`items.slice(start, stop)` returns `Result[list[T], AllocError]` for a
forward list slice. Both arguments are required `i64` indices; start is inclusive
and stop exclusive. Negative bounds count from the end, bounds clamp to
`[0, len(items)]`, and a stop before start yields an empty list. Extreme signed
bounds cannot overflow. The receiver, start, and stop are observed once in that
order, including reference arguments. Slice syntax, omitted bounds, and steps
are deferred.

The result is a new list. Immutable elements are retained, with no deep copy;
its lifetime is independent of the source. Affine elements require an owned
temporary receiver and transfer only the selected owners. The temporary's other
elements are dropped in source order after the call; on failure all its elements
are dropped. Use `copy(items)?.slice(start, stop)` for explicit duplication
that preserves an affine source. Borrowed sources remain unchanged on failure.
The complete output buffer and header are reserved before any transfer (two
allocations for nonempty slices, one for empty slices). Capacity/layout overflow
and exhaustion return `AllocError`; input construction and user cleanup retain
their own policies.

`range(stop)`, `range(start, stop)`, and `range(start, stop, step)` exclude stop
and return a range directly, without a Result or heap allocation. Ranges store
only start/stop/step/length, copy as small independent values, and support repeated
iteration. Passing and returning them, including through Option/Result, require
no heap allocation. Collecting their elements into a list or set remains fallible.
`range[T](...)` chooses any integer type;
start and stop have type T, while step is always signed i64, permitting descending
unsigned ranges. Without explicit type arguments, a typed bound or range
annotation supplies T, otherwise it defaults to i64. Bounds must fit T, so an
exclusive u8 stop of 256 is rejected; choose a wider range for that boundary.
A zero step or length exceeding i64 is a runtime error. Bounds and length use
wider intermediate arithmetic, including the full u64 domain. Range membership is constant
time. `%` uses the divisor's sign, like Python; division by zero is an error,
while `INT_MIN % -1` is zero.

For a directly written range in a list/set/dict comprehension, a numeric output
annotation can guide its loop variable through direct arithmetic in the output
expression. For example `squares: list[u8] = [n * n for n in range(8)]?` selects
u8. Function return types supply the same context. This is bounded expression
inference, not whole-program inference: constraints are not inferred backward
through arbitrary calls, filters, stored range bindings, or explicit range type
arguments. Use `range[u8](...)` at those boundaries. Arithmetic retains the
existing fixed-width overflow rules.

`for name in iterable:` evaluates its iterable once and consumes owned collections
and generators. `for name in &collection` borrows instead; owned list elements
become shared references. Borrowed generator iteration is rejected; `next` accepts an
exclusive generator reference. An explicit `copy(collection)` provides a snapshot
when mutation of the original is needed during iteration. In each form,
lists yield elements, dictionaries keys, sets elements, ranges integers, and
strings Unicode scalar values as `str`. Loop variables and declarations are
block-local; iteration bindings are immutable and explicit `mut` declarations
remain permitted. Updates to enclosing mutable bindings persist. Even
a body that always returns cannot prove a loop executes, so function return
checking retains the zero-iteration path. Loops have unit value.

`while condition:` checks a Boolean condition before each iteration. Its body
has the same binding scope as a `for` body. `break` exits the innermost loop;
`continue` skips the rest of that iteration. In a `for`, continuing advances
the iterator exactly once; in a `while`, it reevaluates the condition. Neither
accepts a value or may cross a function boundary. Statements after an
unconditional control-flow exit are rejected. Loop `else` clauses are not
supported. Return checking conservatively retains a fallthrough path even for
`while True`; value-returning functions need a result after the loop.

List, set, and dictionary comprehensions accept multiple `for` and `if`
clauses. Clauses nest left to right; each iterable is evaluated when its enclosing
iteration reaches it. Filters run before the result expression. Dictionary keys
are evaluated before their values. Comprehension variables have their own scope,
and the first iterable sees the enclosing scope. Conditional expressions in
iterables or filters must be parenthesized. No generator expressions, async
iteration, arbitrary iterator protocol, or user special methods. Flat tuple
unpacking works in loop and comprehension targets.

`for key, value in dictionary.items()` borrows the named dictionary for the loop
and preserves insertion order. Keys and copyable values are loaded as values;
owned values become shared references. `(&mut dictionary).items()` (also
`&mut dictionary.items()`) yields mutable value references and immutable key
values. No item tuples or snapshot buffers are allocated for these loops.
`items()` is currently a loop/comprehension intrinsic rather than a storable
iterator. A snapshot can be built explicitly with a comprehension, using `copy`
or `copy` when values are owned. Dictionary mutations that could invalidate
iteration conflict with the source loan.

Python's [display and comprehension rules](https://docs.python.org/3/reference/expressions.html#displays-for-lists-sets-and-dictionaries)
inform evaluation order and scope; [dictionary semantics](https://docs.python.org/3/library/stdtypes.html#mapping-types-dict)
inform key iteration, replacement, and insertion order. Independent values,
block-local loop variables, and fixed element types are deliberate Plenty choices.
