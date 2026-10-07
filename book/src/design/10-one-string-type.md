# One string type

There is one public `str`: an immutable sequence of Unicode scalar values.
Storage is valid UTF-8 with an explicit byte length and cached scalar count.
Embedded U+0000 is ordinary content; `\0` is a supported literal escape.
Neither literals nor dynamic strings have a trailing terminator. Equality and
hashing include every byte and do not normalize Unicode. `len` counts scalars,
not grapheme clusters; indexing (including negative indices) returns a one-scalar
`str`. Concatenation and indexing return independent values.

`text.isascii() -> bool` is true when every character belongs to ASCII, including
controls and NUL; it is true for empty text. `text.isspace() -> bool` requires at
least one character and all characters to have Unicode's White_Space property,
using the same bundled Rust tables as `strip`. NUL and U+200B are not whitespace.
This is not a promise to reproduce Python's extra whitespace classifications.
Both methods take no arguments, observe their receiver once, accept references,
and allocate nothing.

`text.removeprefix(prefix)` and `text.removesuffix(suffix)` return
`Result[str, AllocError]`, removing at most one exact UTF-8 prefix or suffix.
Empty patterns and missing matches preserve the contents; no normalization,
character-set stripping, or repeated removal occurs. Receiver and argument are
observed once, left to right. One independent output allocation occurs even for
empty or unchanged results, with recoverable failure and unchanged inputs.

`text.startswith(prefix)` and `text.endswith(suffix)` return `bool` without
allocating. Each takes exactly one string (or reference), observes both operands
once in source order, and compares literal UTF-8 without case folding or
normalization. Empty prefixes/suffixes match every string, including empty input.
Optional bounds and tuples of alternatives are not supported. Input expression
construction retains its own allocation policy.

`text.find(needle)` and `text.rfind(needle)` return `Option[i64]` for the first
or last literal match. Positions count Unicode scalars, never UTF-8 bytes;
absence is `Nothing`, including when needle exceeds the source. An empty needle
matches at zero for `find` and at `len(text)` for `rfind`. The last match can
overlap an earlier match. Operands are observed once in source order, references
are accepted, and neither searching nor constructing the inline result allocates.
Converting the byte position to a scalar position scans the preceding prefix.
Optional bounds, regexes, case folding, and normalization are deferred.

`text.count(needle) -> i64` counts literal non-overlapping matches from left to
right without allocating. Empty needles match scalar boundaries, yielding
`len(text) + 1`, including one match in an empty string. No match yields zero.
Both strings are observed once in source order, including references. A source's
validated byte length leaves room for the header, so its boundary count fits i64.
There are no optional bounds, regexes, or normalization.

`text.strip()`, `text.lstrip()`, and `text.rstrip()` return
`Result[str, AllocError]`, removing Unicode White_Space from both ends, the
left end, or the right end respectively. The compiler's bundled Rust runtime
provides the Unicode classification; NUL and zero-width space are not whitespace.
Interior text is preserved byte-for-byte. No explicit character-set argument is
supported. The receiver is observed once, including references. Boundary scanning
does not allocate; creating the independent result requires one allocation even
for empty or unchanged output. Failures preserve the source, and the output
outlives it. No case folding or normalization occurs.

`text.repeat(count) -> Result[str, AllocError]` observes one `i64` count and
the receiver. Positive counts repeat the exact UTF-8 contents; zero and negative
counts produce an empty string, like Python repetition. Empty input produces
empty output for any count without iterating count times. Checked byte/scalar
multiplication and layout validation precede one final allocation, including
empty or single-copy results. Overflow returns `CapacityOverflow`, exhaustion
returns `OutOfMemory`, and the source remains unchanged. The runtime fills the
output by copying and doubling its initialized prefix without intermediate text.
The result owns independent storage. String multiplication syntax is deferred.

`text.slice(start, stop)` returns `Result[str, AllocError]`. It uses the list
slice's two required `i64` bounds, negative indexing, exclusive stop, and clamping,
but positions count Unicode scalars. Reversed bounds produce an empty string.
The receiver and bounds are observed once in source order, including references.
The result owns a new UTF-8 buffer, independent of the source; even empty and
full slices allocate one header/payload buffer. Allocation/layout failure is
recoverable and leaves the source unchanged. The runtime scans scalar boundaries
without an intermediate array, then copies the byte interval. This is linear in
the scanned text length; combining marks remain separate scalars and no Unicode
normalization occurs. Slice syntax and steps remain deferred.

`text.concat(other)` and `separator.join(parts)` return
`Result[str, AllocError]`. Both observe their inputs; `other` must be a `str`,
and `parts` must be a `list[str]` (or a reference to one). Empty list displays
receive that contextual type. Arbitrary iterables/generators are not accepted
yet. Receivers and arguments are evaluated once in source order, under the usual
borrowing rules. Building the argument itself retains its own allocation policy.

Joining inserts the separator between consecutive pieces, including empty
pieces. An empty list produces an empty string; a one-element list produces its
contents without a separator. The runtime first computes checked byte and scalar
lengths, then allocates one final header/payload buffer and copies the exact UTF-8
bytes. This currently includes empty and singleton results. There is no intermediate
text buffer or allocated array of pieces. Length/layout overflow returns
`CapacityOverflow`, allocator rejection returns `OutOfMemory`, and the inputs
remain unchanged. Error transport uses the allocation-free standard sum ABI.
String `+` and indexing return `Result[str, AllocError]`. Index bounds still
trap; `get` returns `Result[Option[str], AllocError]` for recoverable absence.
String iteration yields `Result[str, AllocError]` per scalar, and advances its
byte cursor without allocation even after an error. String literals are immortal
and need no allocation. Ranges currently have heap-backed storage: `range(...)`
and `range[T](...)` return `Result[range[T], AllocError]`.

`text.replace(old, new)` returns `Result[str, AllocError]`. It requires two
`str` arguments (references are accepted) and observes receiver, old, and new
once in that order. Replace every literal, non-overlapping match from left to
right; inserted text is not searched again. An empty old string matches every
Unicode-scalar boundary, including both ends; replacing in an empty string with
an empty pattern inserts new once. Empty new strings remove matches. There is no
count limit, regex interpretation, grapheme matching, or normalization.

Count matches and check the final byte/scalar lengths and object layout before
allocating. Only the final output buffer is allocated, even for empty results,
unchanged results, or zero matches. Byte copying uses a second match scan, with
no intermediate strings, lists, or arrays of match positions. Overflow returns
`CapacityOverflow` and allocation failure returns `OutOfMemory`. All inputs remain
unchanged on either outcome, and successful output outlives them independently.
Input construction retains its own allocation policy.

`text.split(separator)` returns `Result[list[str], AllocError]`, observing
both strings (including references) without consuming them. The explicit separator
must be nonempty: an empty separator is an invalid-operation runtime error, like
an out-of-bounds index, and terminates without unwinding. `AllocError` reports
allocation failure, not invalid arguments. Whitespace splitting with an omitted
separator and a maximum-split argument are not implemented.

Matches are literal, non-overlapping, and scanned left to right. Leading, trailing,
and adjacent separators produce empty pieces; an empty input produces `[""]`.
No match produces a one-element list containing the input's contents. Multibyte
separators and embedded NUL bytes work without normalization. The runtime counts
pieces without allocating, reserves the result list, then allocates each piece's
UTF-8 storage independently. Even empty pieces currently allocate. A failed
allocation reclaims the list and every completed piece without changing either
input; successful pieces remain valid after the original strings are dropped.
The compiler supplies immutable result metadata; no descriptor allocation or
intermediate array of substrings is needed. Input expression construction and
ordinary indexing retain their existing failure policies.

`text.get(index)` returns `Result[Option[str], AllocError]`. The `i64` index
counts Unicode scalars, with negative indices relative to the end, just like
ordinary indexing. An out-of-range index (including either extreme `i64` value)
returns `Ok(Nothing)` without allocating. A valid index returns
`Ok(Some(character))`, using one checked allocation for the scalar's independent
UTF-8 string; allocator rejection returns `Err(AllocError.OutOfMemory)`. The
`Result` and `Option` wrappers themselves never allocate. The source is observed
and remains unchanged, and a successful character outlives it. Receiver and index
are evaluated once in source order; references to either input are accepted.
Lookup takes linear time to reach the scalar within UTF-8 storage; it does not
build a temporary character array. Ordinary `text[index]` shares this runtime
implementation but still traps on missing indices or allocation failure.

The native value is one pointer to a 32-byte prefix followed by exactly the UTF-8
payload: the 16-byte managed header, a u64 byte length, and a u64 scalar count.
Literal headers are aligned to eight bytes and immortal. There is no public
owning/view string distinction. Legacy input validates UTF-8, rejecting malformed
sequences and preserving embedded NUL. Future FFI adapters must explicitly
convert to pointer/length or temporary terminated C text; C-text export must
reject embedded NUL when the external API cannot represent it.
