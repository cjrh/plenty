# Iterable values and borrowing

`for name in iterable:` evaluates the iterable once. Owned collections and
generators transfer into the loop's hidden owner; it drops on exhaustion or
early exit. There is no implicit snapshot. Ranges, strings, and copyable
elements retain their ordinary copy semantics.

Iterating a range, as the source of `for`, `reversed`, `enumerate`, `zip`, a
comprehension, or a generator expression, compiles to a counted loop. The
element count is computed once, before the first iteration, and no step calls
the runtime, so it costs no more than the equivalent `while` loop. This holds
for a `range(...)` call written directly as the source and for a range value
stored in a variable, field, or generator frame or returned by a call. The loop
copies the range when it starts: changing a bound variable, or assigning a new
range to the variable, inside the body does not change the count. A generator
expression's outer source is stored in its frame, so each element also costs
one frame resume.

| Iterable | Yielded value |
| --- | --- |
| List | Element |
| Dictionary | Key, in insertion order |
| Set | Element, with no promised order |
| Range | Integer of the range's element type |
| String | Unicode scalar value as `str` |
| [`bytes`](../03-types/bytes.md) | Byte as `u8` |
| [`std.bytes.Bytes`](../../stdlib/bytes.md#index-syntax-and-iteration) | Byte as `u8` |
| Generator | Declared yielded value |

Consuming list iteration transfers affine elements. `for item in &list`
preserves the list and always yields `&T`, including scalars and strings.
`for item in &mut list` yields `&mut T`, including scalar elements.
Existing collection references follow the same access mode. This includes
direct reference-returning calls: `for item in owner.column()`
and `iter(owner.column())` need no intermediate reference binding. The call
runs once when the source is prepared, and its returned loan protects the owner
through the loop, including backedges, against
invalidating mutation, movement, or destruction. Element loans follow the
[borrowing rules](../05-ownership/02-borrowing.md): only distinct literal list
indices are disjoint. Iteration
references cannot escape into stored references.

Borrowed temporary iterable owners remain alive through the loop and drop
when its scope exits, including on early exit.

Borrowed dictionary and set iteration yields shared references to keys or
elements. Borrowed range, string, `bytes`, and `Bytes` iteration also yields shared
references; each refers to a computed value in an iteration-local slot and
cannot escape that iteration. `for b in &mut buffer` is rejected for `Bytes`;
write through an index instead. Comparisons and printing observe references uniformly;
dereference explicitly when passing an element to a function that takes a value
or storing it in a new collection. Other arithmetic observes numeric references
as well.
Borrowed generators cannot be loop sources; advance one with `next(&mut g)`
instead. Use `for item in copy(values)?` for an explicit independent snapshot
when the original must be mutated during iteration.

`for key, value in dictionary.items():` borrows a named dictionary or field
and preserves insertion order. Keys are `&K` and values are `&V`, regardless
of whether their types are copyable. `(&mut dictionary).items()` or
`&mut dictionary.items()` instead yields `&mut V` values with shared `&K`
keys. These loops allocate
no item tuples or snapshot buffers. `items()` is a loop/comprehension intrinsic,
not a storable iterator, and requires two distinct binding names (or `_`).
Its source loan prohibits invalidating dictionary mutation.

`dictionary.keys()` creates a bindable local `KeyView[K]`; iteration yields
`&K` in the same insertion order, without allocation. `dictionary.values()`
is a loop/comprehension source yielding `&V`, including affine values;
`(&mut dictionary).values()` yields `&mut V`. Both protect the whole dictionary
under their loans. Collect explicitly (`[*key for key in d.keys()]?`), or
use `keys_snapshot()` / `values_snapshot()` for owned lists and `into_keys()` /
`into_values()` to consume the owner. See [dictionary views](../../facilities/01-builtins/dictionaries.md#snapshots-and-views)
for the key view's set operations and storage restrictions.

Loop targets permit flat tuple unpacking. Iteration itself, including string
scalar production and generator `next`, adds no allocation; iterable creation
and body expressions keep their own contracts. There is no arbitrary user
iterator protocol, special-method dispatch, or async iteration.
[Generator expressions](03-generators.md#generator-expressions) construct
lazy owned frames and follow the generator iteration rules.
