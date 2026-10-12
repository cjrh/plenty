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

`dictionary.keys() -> Result[list[K], AllocError]` and
`dictionary.values() -> Result[list[V], AllocError]` take no arguments and
preserve insertion order. Each returns a new list, retaining immutable element
storage. Observed dictionaries remain unchanged on failure, and snapshots
survive later source mutation or destruction.

`keys()` can observe a dictionary with affine values. `values()` with affine
payloads instead requires an owned temporary; its values transfer and the
temporary is consumed on success or failure. Use `copy(d)?.values()` to preserve
such a source explicitly. Both snapshots reserve their complete output before
transfer: two allocations when nonempty, one for an empty owner.

`dictionary.into_values() -> Result[list[V], AllocError]` consumes a named owner
or owned expression directly, including dictionaries with affine payloads.
It preserves insertion order and transfers those payloads without copying.
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
