# Built-in types and operations

These operations are available through the prelude or compiler-known type and
method names. Their contracts complement the language rules for
[types](../../language/03-types/index.md),
[ownership](../../language/05-ownership/index.md), and
[iteration](../../language/09-iteration/index.md).

- [Numbers](numbers.md): parsing, checked integer conversion, and scalar
  formatting.
- [Formatting](formatting.md): representations and printed values.
- [Tuples](tuples.md): fixed products and component access.
- [Boxes](boxes.md): explicit heap ownership and content access.
- [Text](text.md): Unicode values and character lookup.
- [Text search](text-search.md), [building](text-building.md), and
  [splitting](text-splitting.md).
- [Collections](collections.md): common behavior, followed by
  [construction and copying](construction-copy.md), [lists](lists.md),
  [dictionaries](dictionaries.md), and [sets](sets.md).
- [Ranges](ranges.md): allocation-free integer sequences.

An operation's allocation guarantee excludes constructing its arguments and
running user cleanup. `Result` and `Option` wrappers add no allocation. Operations
that observe inputs leave them available; owned arguments follow ordinary move
or copy rules. References are accepted where an operation observes a value,
unless its contract says otherwise.
