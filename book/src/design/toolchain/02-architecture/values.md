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
queued destruction resumes remain separate costs. Strings, the most common heap
field, are freed during the pass and cause no resumption.

A class with exactly one field and no `__del__` is represented as that field:
same slot, same bytes, no record and no internal address. Constructing it,
reading or moving its field, copying it, and releasing it are the field's own
operations; `Wrap(file)` costs nothing beyond `file`. Its runtime descriptor
carries the field's layout under the class's name, so formatting still prints
`Wrap(x=1)` and columnar lists still see one column. A one-field class with
`__del__` needs the cleanup state word and keeps the record layout, as do
classes with two or more fields. The
[flat class layout proposal](../../../proposals/flat-class-layout.md) records
why the record holds an address and the layout contract C interop depends on.

A field of a class record whose type is a class with a record (after looking
through one-field wrappers) is stored *flat*: its record bytes sit at a fixed
offset in the owner's record, with no slot word of its own. Reading, borrowing
or writing `owner.inner.x` adds constant offsets to the owner's record address;
moving the owner copies its bytes with no address fixes for flat fields. An
all-zero flat record is an empty field, so moving a field out zeroes its bytes
and releasing an empty field does nothing. The rule has these exceptions, which
keep the slot word followed by the record:

- top-level class values (locals, list elements, boxes, captures), because
  runtime operations and sum payloads receive one 128-bit value;
- class payloads of enum variants and tuple components, whose records are not
  class records;
- field-less classes without `__del__`, which have no record bytes to place.

A reference to a class value names the 16 bytes before its record whether or
not a slot word is stored there: readers use `address + 16` without loading
the word, writers copy the record bytes, and writing an empty value zeroes
them. A `__del__` hook receives such a reference and must not read the word.
The runtime release walk clears a finished flat record so a resumed walk skips
it.

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
