# Byte literals and `bytes`

A byte literal spells fixed binary data: protocol magic numbers, framing, test
fixtures, search needles. Its type is the built-in immutable type `bytes`, the
byte counterpart of `str`.

```plenty
def is_png(header: bytes) -> bool:
    header == b"\x89PNG\r\n\x1a\n"

def main() -> Result[(), Failure]:
    request = b"GET / HTTP/1.1\r\n"
    print(len(request), request[0], request[-1])?
    print(is_png(b"\x89PNG\r\n\x1a\n"))?
    print(b'single "quotes"')?
    Ok(())
```
```output
16 71 10
True
b"single \"quotes\""
```

## Spelling

A byte literal is `b` directly followed by a quoted string: `b"..."`, `b'...'`,
`b"""..."""`, or `b'''...'''`. Triple quotes permit physical newlines, as for
strings. Each rule has one form:

- The prefix is a lowercase `b`. `B"..."` is rejected with a diagnostic.
  `b` followed by anything other than a quote is an ordinary name, so `b` and
  `B` remain usable identifiers.
- `bytes` is a built-in type name like `str`: no function, type, or import
  binding may take it, so write `import std.bytes`, not
  `import std.bytes as bytes`. A local variable may still be named `bytes`;
  the type has no associated functions, so `bytes.name` always refers to that
  value.
- The text holds only ASCII characters. Any other character is rejected at its
  position with a hint to use `\xHH` escapes or `std.bytes.from_text`. A source
  character never stands for its UTF-8 encoding.
- Escapes are those of string literals (`\0`, `\n`, `\r`, `\t`, `\\`, `\'`,
  `\"`) plus `\xHH`, exactly two hexadecimal digits of either case, for any
  byte. `\u`, `\N`, and every other escape are errors.
- There are no other prefixes: no raw form (`rb`, `br`) because there are no raw
  strings, and no formatted form (`bf`, `fb`). Build formatted text with a
  string and convert it with `std.bytes.from_text`.
- Adjacent literals are not concatenated, as for strings.
- A byte literal is not a `match` pattern; compare with `==`.

## Values

A `bytes` value refers to read-only data emitted into the executable. Creating,
copying, passing, returning, storing, or comparing one never allocates, and it
has no owner to release: assignment copies the reference, like an integer. The
contents cannot change.

| Operation | Result |
|---|---|
| `len(value)`, `value.len()` | Number of bytes, `i64` |
| `value[i]` | The byte at `i`, `u8`; list index rules: negative counts from the end, a missing index stops the program with `index out of bounds` |
| `for byte in value`, `reversed(value)` | Each byte as `u8` |
| `x in value` | Whether the `u8` `x` occurs |
| `==`, `!=` | Content equality with another `bytes` |
| `print(value)` | The literal spelling, printable ASCII with `\xhh` for every other byte |

`bytes` values can be elements, set members, dictionary keys, class fields, and
enum payloads, and can be captured and passed to scoped threads. `len` is the
only method; it lets `bytes` satisfy a [protocol](../07-generics/protocols.md)
that requires it. There are no ordering comparisons, no concatenation, and no
slicing. `bytes` and
`str` are distinct: neither converts to the other implicitly.

## Owned buffers

[`std.bytes.Bytes`](../../stdlib/bytes.md) is the owned, growable buffer.
`std.bytes.from_bytes(value)` copies a literal into a new `Bytes` and returns a
`Result`, because the copy allocates.

Operations that only read bytes, such as `find_bytes`, `matches`, `extend`, and
every `std.io`, `std.net`, and `std.tls` stream write, take a
[`std.bytes.ByteSource`](../../stdlib/bytes.md#byte-sources): a generic `&S`
satisfied by both `bytes` and `Bytes`. A literal passed there is read in place;
nothing is copied or converted, and there is no implicit conversion between the
two types. Changing or growing bytes needs a `Bytes`.

The literal type is a separate prelude type rather than a static `&Bytes` view
so that the prelude does not depend on `std.bytes`: every program would
otherwise know the library class. A generic protocol, rather than a third
read-only view type, is what lets one parameter accept both.
