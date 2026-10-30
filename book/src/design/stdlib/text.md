# `std.text`

Bounded reusable text construction, joining, scalar wrapping, and byte-set
scans. Import `std.text`; every allocating operation returns
`Result[..., TextError]`.

| Operation | Behavior |
|---|---|
| `builder(max_bytes: i64)` | New empty `Builder`; a negative limit is `InvalidLimit` |
| `Builder.byte_len()`, `Builder.limit()` | Current UTF-8 byte count and configured maximum |
| `Builder.append(text: &str)` | Append a copy, or leave contents unchanged on failure |
| `Builder.clear()` | Empty the builder, retaining its allocation |
| `Builder.to_text()` | Independent UTF-8 string snapshot; builder remains reusable |
| `Builder.finish()` | Final UTF-8 string; consumes the builder on success or failure |
| `join(pieces: &list[str], separator: &str, max_bytes: i64)` | Separator between pieces, including empty pieces; empty list yields empty text |
| `wrap(text: &str, width: i64, max_bytes: i64)` | Hard wrap at `width` Unicode scalars, inserting LF; width must be positive |
| `find_in[S: ByteSource](text: &str, set: &S)` | `Option[i64]`: scalar position of the first ASCII scalar whose byte is in `set` |
| `find_not_in[S: ByteSource](text: &str, set: &S)` | `Option[i64]`: scalar position of the first scalar that is not such a member: the end of a run |

`TextError` has `InvalidLimit`, `LimitExceeded`, `Allocation(AllocError)`, and
`Bytes(BytesError)` variants. Byte-buffer failures preserve their typed cause;
normalization uses the direct allocation variant. Limits apply to output bytes,
not characters. Appending checks the complete input size without allocating,
then encodes a temporary buffer and appends atomically. Invalid UTF-8 must be
handled at [byte decoding](bytes.md); `str` is always valid UTF-8. Embedded NUL
is preserved. Builders cannot expose or mutate incomplete UTF-8.

Wrapping preserves every input scalar and existing LF, including final LF.
It never inserts an extra LF before an existing LF. It does not collapse spaces,
find word boundaries, calculate terminal display width, or keep grapheme clusters
together: a combining mark can begin a new line. Width and string positions are
scalar counts; the output limit and `byte_len` are byte counts.

### Segmentation and display width

The official [`unicode_text`](https://github.com/plentylang/unicode-text)
package provides [UAX #29](https://www.unicode.org/reports/tr29/) grapheme
cluster, word and sentence boundaries, and terminal display width following the
Rust `unicode-width` crate, with Unicode 17.0.0 data matching
[`std.unicode`](unicode.md). It is plain Plenty; add it as a
[git dependency](../toolchain/01-building/packages.md#adding-dependencies):

```toml
[dependencies]
unicode_text = { git = "https://github.com/plentylang/unicode-text.git", tag = "v0.1.0" }
```

```plenty fragment
import unicode_text
from unicode_text import Ambiguous

def main() -> Result[(), AllocError | IoError]:
    text = "Hi 👩‍🔬! 日本語のテキスト。"
    print(unicode_text.grapheme_list(&text)?)?
    print(unicode_text.word_list(&text)?)?
    for span in unicode_text.sentences(text):
        print(span.start, span.stop)?
    print(unicode_text.width(&text, Ambiguous.Narrow))?
    print(unicode_text.width("±1", Ambiguous.Wide))?
    Ok(())
```

```text
["H", "i", " ", "👩‍🔬", "!", " ", "日", "本", "語", "の", "テ", "キ", "ス", "ト", "。"]
["Hi", "日", "本", "語", "の", "テキスト"]
0 8
8 17
25
3
```

`graphemes`, `word_bounds`, `words` and `sentences` are generators of `Span`
values carrying scalar positions and UTF-8 byte offsets, and allocate nothing;
the `*_list` forms return `Result[list[str], AllocError]`. `width` takes an
explicit `Ambiguous.Narrow` or `Ambiguous.Wide` for East Asian Ambiguous
characters and, like `unicode-width`, counts each control scalar as one column;
`scalar_width` returns `Nothing` for a control. The package README lists the
complete API, policies and data update procedure.

`find_in` and `find_not_in` are the `str` forms of the
[`Bytes` scans](bytes.md#scanning): `set` is a `Bytes` buffer or a `bytes`
literal, borrowed, and the whole text is scanned in one native call that
allocates nothing. Only ASCII scalars can be members; set bytes from `0x80`
match nothing, so a non-ASCII scalar never matches `find_in` and always stops
`find_not_in`. Results are scalar positions, as for `str.find`, and there are no
bounds arguments. Empty text gives `Nothing`; an empty set makes `find_in`
`Nothing` and `find_not_in` `Some(0)` for nonempty text. Use them to validate a
character class without a per-scalar loop:

```plenty
import std.text

def token(text: &str) -> bool:
    text.byte_len() > 0 and std.text.find_not_in(text, b"!#$%&'*+-.^_`|~0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz").is_none()

def main() -> Result[(), AllocError | IoError]:
    print(token("GET"), token("G T"), token("GÉT"), std.text.find_in("a=b", b"="))?
    Ok(())
```
```output
True False False Option[i64].Some(1)
```

`find_in` costs O(text bytes), plus counting the scalars before a match;
`find_not_in` costs O(bytes up to the first non-member).

The builder owns a compact `Bytes` buffer. `finish(self: Builder)` consumes it;
use `to_text` if the builder must survive an allocation failure for retry or
remain available for reuse. Both operations currently copy the bytes into a
new string; `finish` does not promise zero-copy conversion. Other methods borrow it; `append` and
`clear` require exclusive access, and ordinary loan checking rejects mutation
while shared access remains live. There is no global mutable state or thread
affinity. Appending takes O(input bytes) amortized time and a temporary copy of
that input; growth can temporarily retain old and replacement allocations.
Storage capacity grows geometrically and may exceed the logical byte limit;
the limit is not an exact resident-memory quota. `to_text` copies O(output bytes).
Joining and wrapping take O(input + output bytes) amortized time, plus output
storage. Wrapping currently allocates a small temporary byte buffer per scalar.

Algorithms are checked Plenty using `std.bytes`; no new OS backend is required.
Sources and storage primitives are bundled for offline compilation with a
relocated matching toolchain on x86_64 Linux GNU. See the
[runnable lesson](../../tutorial/08-resources/25-text-unicode.md).

## Acknowledgements

- `Builder` is modelled on Go's [`strings.Builder`](https://pkg.go.dev/strings#Builder); `join` follows Python's [`str.join`](https://docs.python.org/3/library/stdtypes.html#str.join).
