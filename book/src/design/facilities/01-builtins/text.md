# Text

`str` is immutable valid UTF-8 representing Unicode scalar values. Embedded
U+0000 is ordinary content (`\0` is a literal escape). Equality and hashing
compare all bytes without normalization; strings have no public terminator.
Ordering (`<`, `<=`, `>`, `>=`) is lexicographic by Unicode scalar value,
matching the order used by `list[str].sort()`. Prefixes precede longer strings;
comparison observes both inputs and allocates nothing.
`len(text)` counts scalars, including combining marks individually, rather than
bytes or grapheme clusters. `text.byte_len() -> i64` is the length of the UTF-8
encoding in bytes: `len("héllo")` is `5` and `"héllo".byte_len()` is `6`. It
observes the receiver, takes constant time, and allocates nothing; use it to
enforce a size limit before copying or encoding text. `str` has no `len`
method, so the two counts cannot be confused at a call site.

String literals require no allocation. Results of at most seven UTF-8 bytes,
including every single character and the empty string, also require no heap
allocation. Longer constructed results use fallible storage. This threshold does
not change the public type or the result type of an operation.

## Character lookup

`text[index] -> str` accepts an `i64` scalar index, with negative indices counted
from the end. A missing index traps. `text.get(index) -> Option[str]` instead
returns `Nothing`, including for either extreme `i64` index.

Both observe the text and index once in source order, accept references, and
produce an independent single-scalar string without allocation. ASCII lookup is
constant-time; other text requires scanning to the scalar boundary. String
iteration likewise yields one-scalar strings without allocation.

`ord(text) -> Result[u32, ParseError]` converts a one-scalar string to its
Unicode code point. Empty text or multiple scalars return `ParseError.Invalid`.
`chr(code: u32) -> Result[str, ParseError]` performs the reverse conversion;
surrogate code points and values above U+10FFFF return `ParseError.OutOfRange`.
Both allocate nothing, including for non-ASCII scalars. Unsuffixed `chr`
arguments infer `u32`; other integer widths require explicit conversion.
Indexing and iteration retain the existing single-scalar `str` representation;
there is no separate `char` type.

## Classification

`text.isascii() -> bool` is true for empty text and for every ASCII character,
including controls and NUL. `text.isspace() -> bool` requires nonempty text whose
characters all have Unicode's White_Space property. NUL and U+200B are not
whitespace. The bundled Unicode tables also govern trimming; Python's additional
whitespace classifications are not promised.

`text.isdigit()` and `text.isalpha()` require nonempty text consisting entirely
of ASCII digits (`0`–`9`) or ASCII letters (`A`–`Z`, `a`–`z`), respectively.
They deliberately do not implement Python's Unicode digit/letter predicates.

All classification methods observe their receiver, take no arguments, and allocate nothing.

`text.lower()` and `text.upper()` return `Result[str, AllocError]`, applying
Unicode's per-scalar case mappings without locale-specific or contextual rules.
Mappings may expand a scalar (`ß` becomes `SS` in uppercase). They observe the
receiver and allocate only the final output, fallibly; short results stay inline.
They do not normalize or implement case folding.
Other operations are grouped under [search](text-search.md),
[building](text-building.md), and [splitting](text-splitting.md).
