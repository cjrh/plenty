# Collections

For primitive-field class records scanned by field, see
[columnar lists](clists.md). Their packed columns and borrowed scalar pipelines
have a deliberately smaller interface than ordinary lists.

`list[T]`, `dict[K, V]`, and `set[T]` have concrete element types, including
nested collections. Dictionary keys and set members are integers, `bool`, `str`,
`bytes`, or tuples recursively containing those types. Hashing uses tuple
contents, including string and byte-literal contents, independently of where a
tuple is stored. Floats, owned collection/class components, and user-defined
hash/equality are unsupported. Unit elements are rejected.

Dictionary and set bucket selection uses SipHash-2-4 over the complete key,
including framed tuple fields and string lengths. One secret 128-bit key is
obtained from OS entropy on the process's first hash operation and shared across
worker threads. Initialization and hashing allocate no runtime storage.
Constructing or reserving an empty collection does not need entropy. The first
hash may wait for the OS entropy source to initialize. If the OS cannot
provide the key, the operation traps with `collection hashing requires OS
entropy`; there is no predictable fallback. Interrupted reads retry.

Randomized bucket placement makes precomputed hash-flooding inputs harder to
construct; it does not guarantee worst-case constant-time lookup. Hash values
are private implementation details, not persistent identifiers or cryptographic
digests. No test-mode fixed seed is needed: dictionary insertion order is
independent of buckets. Set iteration order remains unspecified.

Lists preserve order and duplicates. Dictionaries preserve first insertion
order: replacing a value keeps its key's position. Sets remove duplicates and
promise no iteration order. Equality is structural: list order matters;
dictionary insertion order and set order do not. Structural comparisons allocate
nothing. Collections have no identity test.

`len(collection) -> i64`, `in`, and `not in` observe collections. Dictionary
membership tests keys. Indexed reads use `i64` list indices (including negative
indices) or the dictionary's key type; missing keys and out-of-bounds indices
[trap](../../language/08-errors/02-allocation-and-traps.md) with exit status
one, reporting the source position. An index is one expression evaluated once; multiple
comma-separated indices are rejected. [`std.bytes.Bytes`](../../stdlib/bytes.md#index-syntax-and-iteration)
uses the list index rules for `buffer[i]` reads and writes.

Collections move on assignment, owned calls, and returns. Independent duplication
uses [copy](construction-copy.md). Indexed reads do not extract affine owners:
use an explicit element reference, an owned removal operation, or copying.
Element loans protect the whole collection unless they use distinct
nonnegative integer literal list indices. See
[borrowing](../../language/05-ownership/index.md) and
[iteration](../../language/09-iteration/index.md).

Flat unpacking also consumes a list: `a, b = values` requires exactly two
elements and transfers their owners; `_` drops an ignored element. A length
mismatch traps at the unpacking expression before any target is written.
Unpacking itself allocates nothing. Borrowed lists, nested targets, and starred
targets are unsupported; explicitly copy a list first to preserve it.

`replace(collection[index], value)` transfers the previous element out while
writing its replacement. It requires exclusive access, evaluates the replacement
before the target index, and adds no allocation. List indices must exist;
dictionary keys must already be present, as for indexed assignment.

## Shared mutation rules

Mutation requires a named `mut` binding, mutable class field, or exclusive
reference. Arguments and indices evaluate before the target is exclusively
borrowed, so `items.append(len(items))` is valid. Active conflicting loans
still reject mutation; temporary mutation receivers are unsupported.

## Copying and borrowed lookup

Optional lookups named `get`, and `peek` for containers with an order such as
caches and heaps, return a copy: `dictionary.get(key) -> Option[V]` and
`list.get(index) -> Option[T]`. Indexing traps on a missing key or index
instead.

A copying `get` has no hidden cost. It accepts only
[copyable values](../../language/05-ownership/01-bindings-moves-and-copies.md):
scalars, immutable strings, ranges, function values, and tuples or enums of
those. Copying one is a fixed-size copy; a string shares its immutable storage.
Neither allocates nor duplicates a buffer. Values that own storage, including
every class instance and every collection, are affine, and `get` rejects them
at compile time ("get cannot return owned dictionary values"), so owned data is
never copied by accident.

Copying is the default because the common case reads a small value and uses it
directly, as in `counts.get(key, 0) + 1`. A borrowing `get` would need a
dereference at each such use, and its `get(key, default)` form could not exist:
returning either the stored value or the default would choose between two
borrows, which [returned references](../../language/05-ownership/04-returned-references.md)
do not allow.

Borrowed forms take a suffix: `get_ref` and `peek_ref` for shared references,
`get_mut` and `peek_mut` for exclusive ones. Builtin and standard library
containers follow the same rule, so the method name tells the reader whether
they hold a copy or a borrow. User functions can return
[`Option[&V]`](../../language/05-ownership/05-optional-references.md), but the
builtin containers do not provide optional borrowed lookups yet.

`collection.clear() -> ()` removes all entries while retaining capacity and
allocating no runtime storage. Removed owners drop before return: lists in
element order, dictionaries in insertion order with keys before values, sets in
unspecified order. Independent immutable owners survive. Empty clear is harmless.

See [construction and copying](construction-copy.md) for constructors, growth,
reservation, and failures, then [lists](lists.md), [dictionaries](dictionaries.md),
and [sets](sets.md) for their operations.
