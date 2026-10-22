# Dictionaries

Dictionaries use the [common collection rules](collections.md).
`dictionary[key] = value` replaces an existing entry without allocation; an
absent key traps. Use [insert](construction-copy.md) to add a key recoverably.
`insert(key, value)` also replaces an existing value, so counting can use
`counts.insert(key, counts.get(key, 0) + 1)?`. This deliberate upsert spelling
exposes allocation failure; indexed assignment remains allocation-free.

## Lookup and removal

`dictionary.get(key) -> Option[V]` observes receiver and key once in source
order, including references. It copies or retains only non-affine values;
collections, classes, and enums containing affine payloads are rejected.
Lookup allocates nothing. Results outlive source replacement or destruction.

`dictionary.get(key, default) -> V` accepts a second positional argument of
the value type and returns it only when the key is absent. Receiver, key, and
default evaluate once in that order; the default is eager even for a present
key. Lookup still requires a copyable value and adds no allocation. Zero,
`False`, and an empty string are present values. This does not insert the key.
A stored `Nothing` produces `Some(Nothing)`, distinct from a missing key.

`dictionary.pop(key) -> Option[V]` requires exclusive access and supports all
permitted value types. A hit removes the entry, drops its stored key, and transfers
the value's owner into `Some`; a miss returns `Nothing` without mutation.
The key is observed once before exclusive receiver access. Removal takes
exactly one key, with no default argument.

Removal allocates nothing, preserves capacity and remaining insertion order,
and appends a reinserted key at the end. Expected lookup/removal cost is constant
with well-distributed keys, plus hashing/comparison and cleanup; adversarial
collisions can make it linear.

## Snapshots and views

`dictionary.keys() -> KeyView[K]` creates a shared, local-only borrowed view
without allocating. A view can be bound to a local, iterated, and used with
builtin set operations. Iteration yields `&K` in insertion order; use
`[*key for key in dictionary.keys()]?` to collect owned keys explicitly.
`len(view)` is O(1), and `key in view` / `not in` use dictionary lookup.
The value holds a borrowed owner handle with a compiler-known key projection;
the owner retains its row descriptor and each loop keeps its own cursor.

`==` and `!=` compare keys as sets, ignoring insertion order, against
`KeyView[K]` or `set[K]` with the same key type. Dictionaries may have different
value types. `issubset`, `issuperset`, and `isdisjoint` take `&set[K]` or
`&KeyView[K]` and return `bool`. These observations allocate nothing and have
no `AllocError`; relationships perform O(n) expected hash lookups (equality
first compares lengths). Empty views are subsets of every set-like operand
and disjoint from every operand.

`union`, `intersection`, `difference`, and `symmetric_difference` take those
same operands and return `Result[set[K], AllocError]`. `to_set()` materialises
the keys as that result type. Algebra reserves its result before inserting
keys and leaves both sources unchanged on failure. Result order is unspecified.
Operators such as `|`, `&`, and `<=` are not part of the view API.

A bound view holds a shared loan on the whole dictionary until its last use.
Insertion, indexed assignment, update, removal, clearing, and moving the owner
conflict with that loan, including through aliases. Views cannot be stored in
fields, collections, tuples, enum payloads, generators or worker/closure
captures, or passed to or returned from user functions. Binding a view of a
temporary dictionary is rejected; immediate iteration may borrow a temporary.
`drop(view)` and `copy(view)` are rejected like ordinary
references; end the loan by finishing the view's last use.
Conditional view bindings are not supported. General stored/returned views
remain separate work.

`dictionary.values()` is a borrowed loop/comprehension source, yielding `&V`
without allocating or copying affine payloads. `(&mut dictionary).values()`
yields `&mut V`. It cannot be bound as a value or used as a set-like operand.

`dictionary.keys_snapshot() -> Result[list[K], AllocError]` and
`dictionary.values_snapshot() -> Result[list[V], AllocError]` take no arguments and
preserve insertion order. Each returns a new list, retaining immutable element
storage. Observed dictionaries remain unchanged on failure, and snapshots
survive later source mutation or destruction.

`keys_snapshot()` can observe a dictionary with affine values. `values_snapshot()` with affine
payloads instead requires an owned temporary; its values transfer and the
temporary is consumed on success or failure. Use `copy(d)?.values_snapshot()` to preserve
such a source explicitly. Both snapshots reserve their complete output before
transfer: two allocations when nonempty, one for an empty owner.

`dictionary.into_keys() -> Result[list[K], AllocError]` and
`dictionary.into_values() -> Result[list[V], AllocError]` consume a named owner
or owned expression directly, including dictionaries with affine payloads.
They preserve insertion order and transfer the selected elements without copying.
The dictionary is consumed on success and failure. Output reservation happens
before any transfer; allocation failure drops the remaining source exactly once.
Use `for value in dictionary.into_values()?:` when the dictionary is no longer
needed. This is an owned snapshot, not a stored borrowed view.

`dictionary.items()` is a borrowed loop/comprehension view rather than a stored
iterator and creates no item tuples or snapshot buffers. Shared views load
shared references to both keys and values, including scalars and strings;
mutable views lend mutable values while keys remain shared references. Its loop loans follow the
[iteration rules](../../language/09-iteration/index.md).

## Transferring an update

`dictionary.update(other) -> Result[(), AllocError]` requires exclusive access
and consumes a same-typed owned dictionary on every outcome. Existing keys keep
their position and key owner; incoming values replace old values. New keys append
in source insertion order.

All storage for absent keys is reserved before replacement or insertion. Failure
preserves destination contents and lookup behavior, though capacity may change,
and drops the source. Success transfers entries without copying; old values drop
in source traversal order after replacement. Empty and replacement-only updates,
or updates fitting reserved capacity, allocate no runtime storage.

Use `update(copy(source)?)` to preserve the source. References, pair iterables,
and keyword updates are unsupported.
