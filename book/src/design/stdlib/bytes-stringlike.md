# Binary string operations

[`std.bytes`](bytes.md) reads arbitrary bytes through `&[u8]`, including byte
literals, `Bytes`, lists of `u8`, and ranges of them. These operations never
decode UTF-8. Positions and lengths count bytes; NUL and bytes above 127 are
ordinary data. Searches and classification allocate nothing.

## Methods and module functions

Byte literals, `&[u8]`/`&mut [u8]` views, and `Bytes` buffers support these
convenience methods. The corresponding public module function takes the source
view first. Method syntax loads its implementation without an explicit import.
A shared slice expression works directly, such as `(&buffer[a:b]).lower()`.
A named exclusive byte view is reborrowed for reading. An explicit
`&mut buffer[a:b]` argument keeps the usual shared-parameter permission
diagnostic; bind that view first or write a shared borrow for a read operation.

| Method | Module function | Result and contract |
| --- | --- | --- |
| `find(needle)` | `find(source, needle)` | `Option[i64]`, first subsequence position |
| `find(value: u8)` | `find_byte(source, value)` | `Option[i64]`, first matching byte |
| `rfind(needle)` / `rfind(value: u8)` | `rfind` / `rfind_byte` | Last matching position |
| `count(needle)` / `count(value: u8)` | `count` / `count_byte` | `i64`, nonoverlapping occurrences |
| `startswith(prefix)` / `endswith(suffix)` | `starts_with` / `ends_with` | `bool`, exact byte prefix/suffix |
| `isascii()` | `is_ascii(source)` | All bytes below 128; true for empty input |
| `isspace()` | `is_space(source)` | Nonempty ASCII whitespace only |
| `isdigit()` / `isalpha()` / `isalnum()` | `is_digit` / `is_alpha` / `is_alnum` | Nonempty ASCII decimal digits, letters, or both |
| `lower()` / `upper()` | `lower(source)` / `upper(source)` | `Result[Bytes, AllocError]`, independent ASCII case-converted copy |
| `strip(set=...)` / `lstrip(set=...)` / `rstrip(set=...)` | Same name, source first | `Result[Bytes, AllocError]`, independent copy with edge bytes removed |
| `split(separator)` | `split(source, separator)` | `Result[list[Bytes], AllocError]`, independently owned fields, preserving empty fields |
| `split()` | `split_whitespace(source)` | `Result[list[Bytes], AllocError]`, runs of ASCII whitespace, no empty fields |
| `replace(old, replacement)` | `replace(source, old, replacement)` | `Result[Bytes, AllocError]`, independent copy with nonoverlapping matches replaced |
| `join(parts)` | `join[T](separator, parts: &list[T])` | `Result[Bytes, AllocError]`, joins byte-readable parts using the receiver as separator |

Needles, prefixes, suffixes, replacements and strip sets take byte views, so
pass literals or borrow an owner/range without converting it. Membership accepts
a `u8` or byte sequence: `0u8 in data`, `b"\r\n" in data`, and `not in` all
work on literals, buffers and byte views. These operators do not turn general
list membership into subsequence searching.

The existing `Bytes.find(value, start, stop)` and
`Bytes.count(value, start, stop)` range methods remain available with their
existing argument validation and absolute indices. The new one-argument forms
search the whole receiver. A view's results are relative to that selected view.
`Bytes.find_bytes(needle, start, stop)` also retains its existing contract.

`find` and `rfind` return `Nothing` when absent. An empty needle matches at zero
for `find`, at `len(source)` for `rfind`, and at every byte boundary for `count`.
An empty prefix or suffix matches. ASCII whitespace is space, tab, LF, CR,
vertical tab (`0x0b`) and form feed (`0x0c`). Default stripping uses that set;
an explicit set is a collection of removable bytes, not a substring.

ASCII case conversion changes `A`–`Z` or `a`–`z` only. High bytes remain
unchanged even if they happen to encode Unicode text. These operations provide
no Unicode classification, normalization, or grapheme segmentation.

## Copies, allocation and joining

Every transformation produces independently owned byte storage; it never
changes the input or returns a stored/returned borrowed range. Allocation
failure propagates as `AllocError`; checked buffer growth and join size
calculation report `CapacityOverflow` when a result cannot be represented.
Inputs bound classification, copying, splitting and replacement work; splitting
creates at most one more field than input bytes, and replacement inserts only
at nonoverlapping matches or byte boundaries for an empty old pattern.

`split(b"")` is a caller error and traps with `bytes.split: separator is empty`.
Whitespace splitting of empty/all-whitespace input returns an empty list;
explicit-separator splitting of empty input returns one empty buffer.
Replacing an empty pattern inserts the replacement before, between and after
source bytes. For example, `b"ab".replace(b"", b"-")?` is `b"-a-b-"`.

Joining accepts a borrowed list of byte-readable elements, such as
`list[bytes]`, `list[Bytes]` or `list[list[u8]]`. Elements are read in place;
no intermediate encoded strings are created. Empty lists produce an empty
buffer and single-element lists insert no separators. A first pass checks the
complete output size; the result then reserves that size and appends the bytes.
The generic parameter must instantiate to a byte-readable element type;
string elements require explicit encoding. Iterator-based joining is not part
of this interface.

```plenty
def main() -> Result[(), AllocError | IoError]:
    wire = b"  NAME=\xff\x00  "
    print(b"=" in wire, wire.find(b"="))?
    clean = wire.strip()?.lower()?
    fields = clean.split(b"=")?
    print(b":".join(&fields)?)?
    Ok(())
```
```output
True Option[i64].Some(6)
b"name:\xff\x00"
```

Forward subsequence search reuses the existing linear-time byte search.
Reverse search uses the already shipped `memchr` implementation to preserve
linear worst-case time and constant auxiliary space for repetitive inputs.
The remaining operations are implemented directly in Plenty over byte views
and checked buffer primitives.
