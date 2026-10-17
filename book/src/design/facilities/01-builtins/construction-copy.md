# Collection construction and copying

## Constructors and displays

`list[T].new()`, `dict[K, V].new()`, and `set[T].new()` return
`Result[collection, AllocError]`. `with_capacity(capacity: i64)` has the same
result and reserves at least that many entries, including hash storage.
Aliases expose these constructors too. Empty construction allocates its owner;
negative counts and impossible layouts return `CapacityOverflow`; exhaustion
returns `OutOfMemory`. Partial storage is reclaimed on failure.

`[elements]`, `{key: value}`, `{elements}`, and typed empty constructors such as
`list[i64]()` also return `Result[collection, AllocError]`. `{}` is a dictionary.
Entries must have identical types. Empty displays require contextual types,
such as the annotation in `names: list[str] = []?`; a typed constructor such as
`list[str]()?` needs none. Nonempty displays infer from their first entry unless
context supplies the type.
Already typed entries never implicitly narrow.

The owner is allocated first, then entries evaluate left to right. A failed
insertion stops evaluation and destroys the initialized prefix; unevaluated
entries have no effects. Nested fallible expressions require their own `?`.
A display's error is always `AllocError`, so `?` on it needs a function
returning `Result[_, AllocError]` or `Result[_, Failure]`; otherwise match the
result and convert the error explicitly.
Propagation leaves the enclosing function, not merely the display. Comprehension
evaluation follows the [iteration rules](../../language/09-iteration/index.md).

## Reservation and insertion

These exclusive methods return `Result[(), AllocError]`:

| Receiver | Operation | Behavior |
| --- | --- | --- |
| Any collection | `reserve(additional: i64)` | Room for at least this many entries beyond current length |
| List | `append(value: T)` | Append one element |
| Set | `add(value: T)` | Insert if absent |
| Dictionary | `insert(key: K, value: V)` | Insert or replace without changing existing key order |

Growth reserves all required storage before mutation. Failure preserves contents,
order, and lookup behavior; capacity may change. Negative or impossible counts
return `CapacityOverflow`. A successful reserve permits the specified number of
subsequent insertions without storage allocation. Existing-key replacement and
duplicate set insertion do not grow storage.

Insertion consumes its arguments on both outcomes; failure drops owned inputs.
Reserve before moving an input that must survive a storage failure. The Result
remains fallible even when no growth is needed.

## Conversion

`list[T].from(source)` and `set[T].from(source)` consume an owned collection,
range, or generator and return `Result[collection, AllocError]`. `list(source)`
and `set(source)` provide inferred conversion; `dict(d)` transfers a dictionary.
Set conversion retains the first occurrence of each permitted member.

Failure destroys partial output, the current element, and the remaining source;
generator effects already performed are not rolled back. Borrowed sources,
string iteration, and pair-iterable dictionary construction are unsupported.

## Explicit copying

`copy(value) -> Result[T, AllocError]` observes its operand once, including
references to `T`. Mutable contents are recursively duplicated; immutable
strings can share storage. Scalars and wholly immutable values need no
allocation. Classes with custom cleanup, aggregates containing them, and
generators are uncopyable.

A failed copy preserves the source and releases newly initialized contents.
Partial records do not run whole-record destructors. Copying follows the
[ownership rules](../../language/05-ownership/index.md); it introduces no mutable aliases.
