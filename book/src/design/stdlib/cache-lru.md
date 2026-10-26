# std.cache.lru

`from std.cache.lru import LruCache` provides a fixed-capacity cache that
evicts its least recently used entry. It is ordinary embedded Plenty code over
a private dictionary and lists. Keys follow the
[dictionary key rules](../facilities/01-builtins/collections.md): integers,
`bool`, `str`, `bytes`, or tuples of those. Values may be any type a list can
store, including owners such as lists and classes with destructors.

```plenty
from std.cache.lru import LruCache

def length(text: &str) -> i64:
    len(text)

def main() -> Result[(), AllocError | IoError]:
    mut names = LruCache[i64, str](2)?
    drop(names.put(1, "Ada")?)
    drop(names.put(2, "Grace")?)
    print(names.get_with(&1, &length))?
    match names.put(3, "Edsger")?:
        case Some(entry):
            key, name = entry
            print("evicted", key, name)?
        case Nothing:
            pass
    print(names.contains(&2), names.get_ref(&1))?
    Ok(())
```
```output
Option[i64].Some(3)
evicted 2 Grace
False Ada
```

## Operations

| Operation | Result | Recency | Contract |
|---|---|---|---|
| `LruCache[K, V](capacity)` | `Result[LruCache[K, V], AllocError]` | | Reserve all storage for `capacity` entries |
| `cache.capacity()` | `i64` | | The construction capacity |
| `cache.len()` | `i64` | | Live entry count |
| `cache.is_empty()` | `bool` | | Whether the count is zero |
| `cache.contains(&key)` | `bool` | Unchanged | Membership test |
| `cache.get_with(&key, &f)` | `Option[R]` | Promotes | Call `f(&value)` and return its result |
| `cache.peek_with(&key, &f)` | `Option[R]` | Unchanged | Call `f(&value)` and return its result |
| `cache.get_ref(&key)` | `&V` | Promotes | Borrow the value; trap if absent |
| `cache.peek_mut(&key)` | `&mut V` | Unchanged | Borrow the value mutably; trap if absent |
| `cache.promote(&key)` | `bool` | Promotes | Mark used; `False` if absent |
| `cache.put(key, value)` | `Result[Option[(K, V)], AllocError]` | Promotes | Insert or replace, reporting the displaced entry |
| `cache.remove(&key)` | `Option[V]` | | Remove and transfer the value |
| `cache.pop_lru()` | `Option[(K, V)]` | | Remove and transfer the least recent entry |
| `cache.clear()` | `()` | | Drop entries from least to most recent |

Mutation, promotion included, requires a mutable owner or exclusive reference.
`f` has type `Callable[[&V], R]`; it may be a named function or a closure.

`put` with a new key makes it the most recent entry. If the cache was full, it
removes the least recent entry and returns it as `Some((key, value))`. With an
existing key, `put` keeps the stored key, replaces the value, promotes the
entry and returns `Some((key, old_value))`. Otherwise it returns `Nothing`.
This follows `push` in the Rust `lru` crate. Compare the returned key with
your own to tell eviction from replacement.

The reference accessors trap on an absent key, like `dictionary[key]`. Test
with `contains` first when absence is possible. Their result borrows the
whole cache until its last use, so the cache cannot change meanwhile.

### Why there is no plain `get`

Plenty cannot return `Option[&V]`: references cannot be stored in enum
payloads. A copying `get(&key) -> Option[V]`, like `dictionary.get`, would
require copyable values. Every method of a generic class instance is checked
when the instance is first used, so one copying method would reject
`LruCache[K, V]` for every value type that cannot be copied, even when `get`
is never called. `get_with` and `peek_with` are generic methods, checked only
when called, so they work for every value type. `get` and `peek` returning
copies will be added once methods are checked only when they are called.

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
cache explicitly with `contains`, `get_with` and `put`.

## Acknowledgements

- Behavior modelled on Python's [`functools.lru_cache`](https://docs.python.org/3/library/functools.html#functools.lru_cache), including no caching at `maxsize=0`.
- API modelled on the Rust [`lru`](https://docs.rs/lru/) crate: `put` reporting the displaced entry follows its `push`, along with `peek`, `promote` and `pop_lru`.
