# Value representations

Classes, tuples, and enum payloads keep fields inline in their owner's storage.
Native slots contain ordinary value bits plus any required inline payload bytes.
Moves and copies relocate private addresses into the destination; addresses never
extend the lifetime of departed frames. Immutable metadata describes field layout,
ownership, relocation, equality, and formatting. Concrete layout facts are cached.

Runtime record traversal advances a running slot offset, including each field's
inline payload and any class cleanup state word. A complete pass takes one width
lookup per field, without allocating an offset table. Hashing, equality, ordering,
formatting, copying, relocation, and retain/release use this traversal. Copy
failure cleanup walks the completed prefix backwards with a decreasing offset.
This bounds offset work per pass; nested payload work and repeated visits while
queued destruction resumes remain separate costs.

Compiled code avoids the runtime walk when the type makes it unnecessary. A
payload with no internal addresses, such as a range, a flat record, or a closure
environment without inline captures, is copied as plain bytes; a sum whose
payloads are all such is copied by testing its tag. An all-zero value is an empty
ownership slot, such as a moved-from local, and its release makes no runtime
call. These paths keep high-frequency helpers, such as error mappers called per
byte, out of runtime operation calls.

References use two native words: an address and packed inline-sum tag-offset
information. They point into typed local, frame, field, or collection storage;
source loans determine when those locations remain usable. Function values are
code addresses without capture environments. Returned-reference analysis caches
precise field-origin summaries when available and otherwise uses conservative
owner projections.

Scalar references read and write only the scalar's declared bytes, whether they
address a packed list element, a local slot, or a projected enum payload. Slot
padding is not part of the scalar value; runtime equality, hashing and formatting
ignore it. Projected writes leave ancestor tags unchanged.

## Lists

Dense lists pack primitive numeric and boolean elements into 1-, 2-, 4- or
8-byte rows with natural alignment. Other rows contain a 16-byte value slot
and its inline payload, with no unused dictionary value slot. The runtime
keeps a 16-byte-aligned buffer, rounding allocation sizes up to whole words;
its exposed length counts initialized bytes. Borrowed scalar indexing uses
the concrete element width to calculate row counts and addresses. General
borrowed iteration obtains the runtime row stride. Native `strided` FFI views
use that same stride, and signed reads extend their declared bytes without a
post-call repair pass. [List storage costs](../../facilities/01-builtins/lists.md#storage-costs)
describe the remaining space used by sum and record elements.

## Strings

A `str` is one 64-bit word. Heap and literal pointers address a 32-byte prefix:
a 16-byte managed header, byte length, and scalar count, followed by UTF-8 bytes
without a terminator. Literal storage is immortal and eight-byte aligned.

A set low bit marks an inline string. Its low byte stores `length << 1 | 1`;
the remaining seven bytes store UTF-8. These values own no heap allocation and
retain/release ignores them. Byte equality, hashing, and printing handle both
representations identically. Source-visible text behavior belongs in
[text operations](../../facilities/01-builtins/text.md).

## Standard sums

`Option` and `Result` use a 128-bit value: a 64-bit terminal payload and a 64-bit
path of binary tags, outermost first. Nested sums add bits without boxing, bounded
by the 64-level type limit. The payload is scalar bits, an owned heap handle, or
a private address of inline data. No numeric sentinel or null pointer is reserved
as a source value. `LoadError` uses three terminal tag bits for seven variants.

Scalar/pointer sums use 16-byte addressable slots. Inline payload bytes follow the
value word; slots grow only when the payload requires it. Native results use
caller-owned storage, and relocation preserves sum tags. Runtime aggregate helpers
receive aligned slot pointers instead of relying on a C `u128` calling convention.
These layouts are private and are not the public FFI ABI.

## Ranges

Ranges contain 32 bytes of bounds, step, and length. Stored ranges use 48-byte
slots: ordinary value storage plus inline range data. Collection relocation and
frame/result moves repair embedded addresses. Range creation and return need no
separate owner allocation; see [ranges](../../facilities/01-builtins/ranges.md).
