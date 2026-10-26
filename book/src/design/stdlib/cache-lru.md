# std.cache.lru

`from std.cache.lru import LruCache` provides a fixed-capacity cache that
evicts its least recently used entry. It is ordinary embedded Plenty code over
a private dictionary and lists. Keys follow the
[dictionary key rules](../facilities/01-builtins/collections.md): integers,
`bool`, `str`, `bytes`, or tuples of those. Values may be any type a list can
store, including owners such as lists and classes with destructors.

```plenty
from std.cache.lru import LruCache

def main() -> Result[(), AllocError | IoError]:
    mut names = LruCache[i64, str](2)?
    drop(names.put(1, "Ada")?)
    drop(names.put(2, "Grace")?)
    print(names.get(&1), names.peek(&4))?
    match names.put(3, "Edsger")?:
        case Some(entry):
            key, name = entry
            print("evicted", key, name)?
        case Nothing:
            pass
    print(names.contains(&2), names.len())?
    Ok(())
```
```output
Option[str].Some("Ada") Option[str].Nothing
evicted 2 Grace
False 2
```

## Operations

| Operation | Result | Recency | Contract |
|---|---|---|---|
| `LruCache[K, V](capacity)` | `Result[LruCache[K, V], AllocError]` | | Reserve all storage for `capacity` entries |
| `cache.capacity()` | `i64` | | The construction capacity |
| `cache.len()` | `i64` | | Live entry count |
| `cache.is_empty()` | `bool` | | Whether the count is zero |
| `cache.contains(&key)` | `bool` | Unchanged | Membership test |
| `cache.get(&key)` | `Option[V]` | Promotes | Copy the value; copyable `V` only |
| `cache.peek(&key)` | `Option[V]` | Unchanged | Copy the value; copyable `V` only |
| `cache.get_ref(&key)` | `Option[&V]` | Promotes | Borrow the value |
| `cache.get_mut(&key)` | `Option[&mut V]` | Promotes | Borrow the value mutably |
| `cache.peek_ref(&key)` | `Option[&V]` | Unchanged | Borrow the value |
| `cache.peek_mut(&key)` | `Option[&mut V]` | Unchanged | Borrow the value mutably |
| `cache.promote(&key)` | `bool` | Promotes | Mark used; `False` if absent |
| `cache.put(key, value)` | `Result[Option[(K, V)], AllocError]` | Promotes | Insert or replace, reporting the displaced entry |
| `cache.remove(&key)` | `Option[V]` | | Remove and transfer the value |
| `cache.pop_lru()` | `Option[(K, V)]` | | Remove and transfer the least recent entry |
| `cache.clear()` | `()` | | Drop entries from least to most recent |

Mutation, promotion included, requires a mutable owner or exclusive reference.

`put` with a new key makes it the most recent entry. If the cache was full, it
removes the least recent entry and returns it as `Some((key, value))`. With an
existing key, `put` keeps the stored key, replaces the value, promotes the
entry and returns `Some((key, old_value))`. Otherwise it returns `Nothing`.
This follows `push` in the Rust `lru` crate. Compare the returned key with
your own to tell eviction from replacement.

### Copying and borrowed access

`get` and `peek` copy the value, like `dictionary.get`; see
[copying and borrowed lookup](../facilities/01-builtins/collections.md#copying-and-borrowed-lookup).
They accept only [copyable values](../language/05-ownership/01-bindings-moves-and-copies.md),
such as integers, strings and tuples of those. A cache may still store
lists, classes and other affine values: each method of a generic class is
checked when it is first called, so only a call to `get` or `peek` with such
a value type is rejected. The error points at the call and names the
rejected instance, such as `LruCache[i64, list[i64]].get`, with a note
that its body cannot move out of indexed storage.

For affine values, borrow instead. `get_ref` and `peek_ref` return
`Option[&V]`; `get_mut` and `peek_mut` return `Option[&mut V]`. All four return
`Nothing` for an absent key. A borrowed result follows the
[optional reference rules](../language/05-ownership/05-optional-references.md):
it borrows the whole cache until its last use, so the cache cannot change
meanwhile. Match it, or use `?` or `unwrap()`, to reach the value:

```plenty
from std.cache.lru import LruCache

def main() -> Result[(), AllocError | IoError]:
    mut pages = LruCache[str, list[str]](2)?
    drop(pages.put("home", ["intro"]?)?)
    match pages.get_mut("home"):
        case Some(lines):
            lines.append("news")?
        case Nothing:
            pass
    match pages.peek_ref("home"):
        case Some(lines):
            print(len(lines), lines)?
        case Nothing:
            print("missing")?
    print(pages.peek_ref("about").is_none())?
    Ok(())
```
```output
2 ["intro", "news"]
True
```

## Capacity

Capacity is fixed. Zero is valid and stores nothing: `put` returns its own
entry as `Some((key, value))` at once, matching `functools.lru_cache` with
`maxsize=0`. A negative capacity, or `i64.MAX`, returns
`AllocError.CapacityOverflow`. Construction reserves storage for
`capacity + 1` entries, so a large capacity costs memory before any entry
is stored.

## Errors and costs

`AllocError` is the only error. Construction allocates everything the cache
will use; it returns `OutOfMemory` or `CapacityOverflow` and releases partial
storage on failure. After construction, no operation allocates. `put` still
returns a `Result`, because insertion into the reserved collections remains
fallible. On failure the cache is unchanged and the incoming key and value
are dropped.

Expected cost is constant for lookup, promotion, insertion, eviction and
removal, apart from hashing and value cleanup. Adversarial key collisions can
make dictionary lookup linear. `clear` and dropping the cache are linear.

The cache stores each key twice: in its private dictionary and in its
recency slots. Keys are always copyable, so this needs no explicit copy.

## Limits

There is no iteration in recency order, resizing, statistics, expiry or
thread-safety contract. A memoizing decorator such as `functools.lru_cache`
is out of scope: Plenty has no decorators. Wrap a function in a
cache explicitly with `get` and `put`.

## Acknowledgements

- Behavior modelled on Python's [`functools.lru_cache`](https://docs.python.org/3/library/functools.html#functools.lru_cache), including no caching at `maxsize=0`.
- API modelled on the Rust [`lru`](https://docs.rs/lru/) crate: `put` reporting the displaced entry follows its `push`, along with `peek`, `promote` and `pop_lru`.
