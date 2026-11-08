# Dictionaries

Dictionaries use the [common collection rules](collections.md).
[`insert(key, value)`](construction-copy.md) adds a key or replaces its
value. Counting uses `*counts.get_or_insert(key, 0)? += 1`, which looks the
key up once; see [lookup and removal](#lookup-and-removal). The `?` exposes
allocation failure.

Dictionaries have no indexed assignment. `dictionary[key] = value`,
`dictionary[key] += n`, and `&mut dictionary[key]` are compile errors whose
messages name `insert` or `get_mut`: key presence is a run-time fact, so a
write through an absent key would trap where `--check` cannot see it.
Reading `dictionary[key]` remains, and traps on an absent key.
Writing through `dictionary[key].field` or calling a mutating method on
`dictionary[key]` is rejected the same way; use `get_mut` for one key, or
[a mutable value loop](#updating-every-value) for every value.

```plenty
def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut stock = {"apples": 3}?
    stock.insert("pears", 5)?
    match stock.get_mut("apples"):
        case Some(count):
            *count -= 1
        case Nothing:
            pass
    print(stock)?
    Ok(())
```

```output
{"apples": 2, "pears": 5}
```

## Updating every value

`for value in (&mut dictionary).values():` lends each value as `&mut V` in
insertion order; `for key, value in (&mut dictionary).items():` adds a shared
`&K` key. `&mut dictionary.items()` is the same loop. Each step reaches the
next entry directly, with no key lookup, item tuple, or copy, so it works for
affine values and allocates nothing. Inside a function taking
`d: &mut dict[K, V]`, write `(&mut d).values()`; `d.values()` stays a shared
loop.

```plenty
class Player:
    name: str
    hp: i64
    items: list[str]

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut players = {"ada": Player("Ada", 3, []?), "lin": Player("Lin", 9, []?)}?
    for player in (&mut players).values():
        player.hp += 10
        player.items.append("potion")?
    for key, player in (&mut players).items():
        if *key == "lin":
            player.hp = 0
            break
    for key, player in players.items():
        print(*key, player.hp, player.items)?
    Ok(())
```

```output
ada 13 ["potion"]
lin 0 ["potion"]
```

The loop holds an exclusive loan of the whole dictionary until it ends, so
the body cannot insert, remove, clear, read, or borrow the dictionary again,
including through `get_mut` or a nested loop; these are `conflicting borrow`
errors. `break`, `return`, and `?` end the loan without dropping or moving any
value. The references are loop-local: storing one in a collection, assigning
it to an outer reference binding, or returning it from a function that does not
take the dictionary as its first reference parameter is rejected. A mutable
projection is an immediate loop source; `(&mut dictionary).values()` cannot be
bound to a name. There is no `values_mut()` or `items_mut()`: the exclusive
receiver selects the mutable loop, as `for x in &mut xs` does for a list, and
those names are compile errors that point here. To change values while also
adding or removing keys, collect the keys first with `keys_snapshot()` and use
`get_mut` per key.

## Lookup and removal

`dictionary.get(key) -> Option[V]` observes receiver and key once in source
order, including references. It copies or retains only non-affine values;
collections, classes, and enums containing affine payloads are rejected.
Lookup allocates nothing. Results outlive source replacement or destruction.
[Copying and borrowed lookup](collections.md#copying-and-borrowed-lookup)
explains why `get` copies and how borrowed lookups are named.

`dictionary.get(key, default) -> V` accepts a second positional argument of
the value type and returns it only when the key is absent. Receiver, key, and
default evaluate once in that order; the default is eager even for a present
key. Lookup still requires a copyable value and adds no allocation. Zero,
`False`, and an empty string are present values. This does not insert the key.
A stored `Nothing` produces `Some(Nothing)`, distinct from a missing key.

`dictionary.get_ref(key) -> Option[&V]` lends the value for `key`, or returns
`Nothing`. Like `get_mut`, it works for every value type, allocates nothing,
and observes the key before borrowing the receiver. The result holds a shared
loan of the whole dictionary until its last use: the dictionary can still be
read, but not mutated or borrowed exclusively. Use it to inspect an affine
value, such as a list or class instance, that `get` cannot copy.

`dictionary.get_mut(key) -> Option[&mut V]` lends the value for `key`
exclusively, or returns `Nothing`. It works for every value type, including
affine ones, and allocates nothing. The key is observed before the receiver is
borrowed. The result holds an exclusive loan of the whole dictionary until its
last use, as a reference-returning method's result does, so the dictionary
cannot be read, mutated, or borrowed again meanwhile. Match on the result to
update a value in place: `*count += 1` for a scalar, `items.append(x)?` for a
list, or `point.x = 0` for a class. `replace(*value, new)` returns the previous
value and keeps the entry's position.

`dictionary.get_or_insert(key, default) -> Result[&mut V, AllocError]` lends
the value for `key` exclusively, first inserting `key` and `default` when the
key is absent. Key and default evaluate in that order before the receiver is
borrowed, and both move into the call. A present key keeps its stored entry
and value; the call drops the incoming key and the unused default before
returning. Either way the key is hashed once, so counting or grouping needs one
lookup per item rather than the two of `get_mut` followed by `insert`. A
missing key may grow the dictionary as `insert` does; on `AllocError` the key
and default are dropped and the dictionary is unchanged. The result holds the
same whole-dictionary loan as `get_mut`'s. The default is eager: it is built
even when the key is present, so prefer `get_mut` when building it costs an
allocation and the key is usually present.

```plenty
def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut counts: dict[str, i64] = {}?
    for word in "to be or not to be".split()?:
        *counts.get_or_insert(word, 0)? += 1
    mut lengths: dict[i64, list[str]] = {}?
    for word in counts.keys():
        group = lengths.get_or_insert(len(*word), []?)?
        group.append(*word)?
    print(counts, lengths)?
    Ok(())
```

```output
{"to": 2, "be": 2, "or": 1, "not": 1} {2: ["to", "be", "or"], 3: ["not"]}
```

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
`[copy(key)? for key in dictionary.keys()]?` to collect owned keys explicitly,
including move-only class keys without custom cleanup. Keys containing `__del__`
can be borrowed here but cannot be copied; use `into_keys()` to transfer them.
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
keys and leaves both sources unchanged on failure. Move-only keys are copied
into independent result storage. All four algebra methods and `to_set()` reject
keys containing custom cleanup, because those keys cannot be copied.
Relationship checks remain available. Result order is unspecified.
Operators such as `|`, `&`, and `<=` are not part of the view API.
Set relationship and algebra methods accept mixed set/view operands in either
receiver order. `set.intersection_update(view)` and
`set.difference_update(view)` also accept key views. Equality accepts either operand order. These builtin
methods observe their operands, so an explicit `&` may be omitted without
moving a named set or view.

`set(view)` and `list(view)` are not supported. Use `view.to_set()`,
`dictionary.keys_snapshot()`, or `[*key for key in view]?` to choose the owned
result explicitly.

A bound view holds a shared loan on the whole dictionary until its last use.
Insertion, `get_mut`, `get_or_insert`, update, removal, clearing, and moving the owner
conflict with that loan, including through aliases. Views cannot be stored in
fields, collections, tuples, enum payloads, generators or worker/closure
captures, or passed to or returned from user functions. Binding a view of a
temporary dictionary is rejected; immediate iteration may borrow a temporary.
`drop(view)` and `copy(view)` are rejected like ordinary
references; end the loan by finishing the view's last use.
Conditional view bindings are not supported. General stored/returned views
remain separate work.

`dictionary.values()` creates a shared local view yielding `&V`, without
allocating or copying affine payloads. Both `values = dictionary.values()`
and `items = dictionary.items()` can be bound and reused in loops; `len(view)`
is O(1). `iter(values)` and `iter(items)` start pipelines, with item callbacks
taking separate key and value parameters. These views have the same local-only
borrow restrictions as key views and do not provide set operations.
`(&mut dictionary).values()` yields `&mut V` in immediate loops; mutable views
cannot be bound. See [updating every value](#updating-every-value).
Automatic box dereferencing applies to `keys()`, `values()` and `items()`,
including nested boxes and borrowed boxed dictionaries. Their loans protect
the boxed owner throughout the observation or loop.

`dictionary.keys_snapshot() -> Result[list[K], AllocError]` and
`dictionary.values_snapshot() -> Result[list[V], AllocError]` take no arguments and
preserve insertion order. Each returns a new list, retaining immutable element
storage. Observed dictionaries remain unchanged on failure, and snapshots
survive later source mutation or destruction.

A snapshot with affine selected elements requires an owned temporary: this
applies to `keys_snapshot()` for move-only keys and `values_snapshot()` for
move-only values. Those elements transfer and the temporary is consumed on
success or failure. Use `copy(d)?.keys_snapshot()` or
`copy(d)?.values_snapshot()` to preserve such a source explicitly. The unselected
side may be affine even when observing a named dictionary. Both snapshots
reserve their complete output before transfer: two allocations when nonempty, one for an empty owner.

`dictionary.into_keys() -> Result[list[K], AllocError]` and
`dictionary.into_values() -> Result[list[V], AllocError]` consume a named owner
or owned expression directly, including dictionaries with affine payloads.
They preserve insertion order and transfer the selected elements without copying.
The dictionary is consumed on success and failure. Output reservation happens
before any transfer; allocation failure drops the remaining source exactly once.
Use `for value in dictionary.into_values()?:` when the dictionary is no longer
needed. This is an owned snapshot, not a stored borrowed view.

`dictionary.items()` creates a shared local borrowed view, with an independent
cursor for each loop, and creates no item tuples or snapshot buffers. Shared views load
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
