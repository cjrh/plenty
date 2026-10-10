# Collections

`list[T]`, `dict[K, V]`, and `set[T]` have concrete element types, including
nested collections. Dictionary keys and set members are integers, `bool`, or
`str`; floats and user-defined hash/equality are unsupported. Unit elements are
rejected.

Lists preserve order and duplicates. Dictionaries preserve first insertion
order: replacing a value keeps its key's position. Sets remove duplicates and
promise no iteration order. Equality is structural: list order matters;
dictionary insertion order and set order do not. Structural comparisons allocate
nothing. Collections have no identity test.

`len(collection) -> i64`, `in`, and `not in` observe collections. Dictionary
membership tests keys. Indexed reads use `i64` list indices (including negative
indices) or the dictionary's key type; missing keys and out-of-bounds indices
trap with exit status one. An index is one expression evaluated once; multiple
comma-separated indices are rejected.

Collections move on assignment, owned calls, and returns. Independent duplication
uses [copy](construction-copy.md). Indexed reads do not extract affine owners:
use an explicit element reference, an owned removal operation, or copying.
Element loans conservatively protect the whole collection. See
[borrowing](../../language/05-ownership/index.md) and
[iteration](../../language/09-iteration/index.md).

## Shared mutation rules

Mutation requires a named `mut` binding, mutable class field, or exclusive
reference. Arguments and indices evaluate before the target is exclusively
borrowed, so `items.append(len(items))` is valid. Active conflicting loans
still reject mutation; temporary mutation receivers are unsupported.

`collection.clear() -> ()` removes all entries while retaining capacity and
allocating no runtime storage. Removed owners drop before return: lists in
element order, dictionaries in insertion order with keys before values, sets in
unspecified order. Independent immutable owners survive. Empty clear is harmless.

See [construction and copying](construction-copy.md) for constructors, growth,
reservation, and failures, then [lists](lists.md), [dictionaries](dictionaries.md),
and [sets](sets.md) for their operations.
