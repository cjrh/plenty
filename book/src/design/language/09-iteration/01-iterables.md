# Iterable values and borrowing

`for name in iterable:` evaluates the iterable once. Owned collections and
generators transfer into the loop's hidden owner; it drops on exhaustion or
early exit. There is no implicit snapshot. Ranges, strings, and copyable
elements retain their ordinary copy semantics.

| Iterable | Yielded value |
| --- | --- |
| List | Element |
| Dictionary | Key, in insertion order |
| Set | Element, with no promised order |
| Range | Integer of the range's element type |
| String | Unicode scalar value as `str` |
| Generator | Declared yielded value |

Consuming list iteration transfers affine elements. `for item in &list`
preserves the list and yields `&T` for affine elements, values for copyable
elements. `for item in &mut list` yields `&mut T`, including scalar elements.
Existing collection references follow the same access mode. Loans last
through the loop, including backedges, and protect the source against
invalidating mutation, movement, or destruction. Element loans protect the
whole collection; different indices are not proven disjoint. Iteration
references cannot escape into stored references.

Borrowed dictionary and set iteration yields their copyable keys or elements.
Borrowed generators cannot be loop sources; advance one with `next(&mut g)`
instead. Use `for item in copy(values)?` for an explicit independent snapshot
when the original must be mutated during iteration.

`for key, value in dictionary.items():` borrows a named dictionary or field
and preserves insertion order. Keys and copyable values are values; affine
values are `&T`. `(&mut dictionary).items()` or `&mut dictionary.items()`
instead yields `&mut T` values with immutable key values. These loops allocate
no item tuples or snapshot buffers. `items()` is a loop/comprehension intrinsic,
not a storable iterator, and requires two distinct binding names (or `_`).
Its source loan prohibits invalidating dictionary mutation.

Loop targets permit flat tuple unpacking. Iteration itself, including string
scalar production and generator `next`, adds no allocation; iterable creation
and body expressions keep their own contracts. There is no arbitrary user
iterator protocol, special-method dispatch, async iteration, or generator
expression.
