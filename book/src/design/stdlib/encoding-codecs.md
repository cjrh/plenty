# Base32, MIME and custom alphabets

These operations extend [`std.encoding`](encoding.md). They are implemented in
Plenty over [`Bytes`](bytes.md), without another runtime primitive or native
dependency. Inputs are whole buffers; there is no streaming state.

```plenty
import std.bytes
import std.encoding
from std.encoding import EncodingError

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | EncodingError]:
    data = std.bytes.from_text("hello")?
    print(std.encoding.base32_text(&data, 0, data.len())?)?
    print(std.encoding.base32_from_text("NBSWY3DP")?.to_text()?)?
    lower = std.encoding.custom("abcdefghijklmnopqrstuvwxyz234567", Nothing)?
    text = lower.text(&data, 0, data.len())?
    print(text, lower.from_text(&text)?.to_text()?)?
    Ok(())
```
```output
NBSWY3DP
hello
nbswy3dp hello
```

## Base32

`base32` uses the uppercase alphabet `ABCDEFGHIJKLMNOPQRSTUVWXYZ234567` from
[RFC 4648 section 6](https://www.rfc-editor.org/rfc/rfc4648.html#section-6).
`base32hex` uses `0123456789ABCDEFGHIJKLMNOPQRSTUV` from section 7. Both require
`=` padding to complete eight-symbol groups. They are case-sensitive and do
not accept whitespace, `0`/`1` substitutions, or nonzero unused trailing bits.

| Function suffix, for both `base32` and `base32hex` | Result |
|---|---|
| `_encode(source: &Bytes, start: i64, stop: i64)` | `Result[Bytes, EncodingError]` |
| `_decode(source: &Bytes, start: i64, stop: i64)` | `Result[Bytes, EncodingError]` |
| `_text(source: &Bytes, start: i64, stop: i64)` | `Result[str, EncodingError]` |
| `_from_text(text: &str)` | `Result[Bytes, EncodingError]` |

For example, `f` encodes as `MY======` or `CO======`. Unpadded forms are
available through `custom` below; a decoder never guesses the padding policy.
The empty input encodes as empty, not a group of padding characters.

Range validation comes first. Padded lengths must be a multiple of eight.
After stripping trailing padding, the number of symbols must describe whole
bytes with fewer unused bits than a symbol. Padding must fill exactly one
partial group. Symbols and trailing bits are then checked in input order.
Impossible lengths produce `InvalidLength`; excessive or misplaced padding
produces `InvalidPadding(index)` when the preceding length checks pass.

## MIME line wrapping

`base64mime_encode`, `base64mime_decode`, `base64mime_text` and
`base64mime_from_text` have the corresponding base64 signatures.

The encoder produces padded standard base64 with CRLF **between** lines of at
most 76 symbols, following the line width of
[RFC 2045 section 6.8](https://www.rfc-editor.org/rfc/rfc2045.html#section-6.8).
It adds no final CRLF and emits nothing for empty input. Add the surrounding
message headers and any final line termination in the enclosing protocol.

The decoder skips only ASCII space, tab, CR and LF. It then applies ordinary
strict base64 length, alphabet, padding and trailing-bit validation. It accepts
unwrapped text and does not enforce a line width. This is deliberately narrower
than RFC 2045's permissive recovery from arbitrary non-alphabet bytes: punctuation,
non-ASCII whitespace, and damaged padding remain errors. Error indexes still
refer to the original buffer, including skipped whitespace; `_from_text` indexes
are UTF-8 byte offsets. It does not parse MIME headers or multipart messages.

## Reusable custom encoding

```text
custom(symbols: &str, padding: Option[u8] = Some(61u8))
    -> Result[Encoding, EncodingError]
```

`symbols` lists digits in increasing value order. It must contain 2, 4, 8, 16,
32 or 64 distinct printable ASCII bytes (`!` through `~`). A padding byte must
also be printable ASCII and must not occur in the alphabet. Invalid settings
return `InvalidAlphabet`. `Some(61u8)` means `=`; `Nothing` forbids padding.
With no configured padding, `=` may itself be an alphabet symbol.

`Encoding` owns its validated tables and has four methods:

| Method | Result |
|---|---|
| `encode(source: &Bytes, start: i64, stop: i64)` | `Result[Bytes, EncodingError]` |
| `decode(source: &Bytes, start: i64, stop: i64)` | `Result[Bytes, EncodingError]` |
| `text(source: &Bytes, start: i64, stop: i64)` | `Result[str, EncodingError]` |
| `from_text(text: &str)` | `Result[Bytes, EncodingError]` |

Bits are read most significant first. Padding completes the smallest group of
symbols representing whole bytes. Decoding accepts only the selected alphabet
and policy, with zero unused trailing bits; it does not fold case, translate
lookalike characters, ignore whitespace, or automatically detect a radix.
If no padding is configured, a byte outside the alphabet is `InvalidByte`,
including `=`. If configured, misplaced occurrences of that padding byte are
`InvalidPadding`.

For a known protocol, prefer the named codec. Construct a custom encoding once
and reuse it when the protocol requires a different alphabet or padding. The
[tutorial](../../tutorial/07-modules/07-standard-encoding.md) demonstrates this.

## Allocation and limits

Base32 and custom encode/decode allocate one result buffer of exactly its final
length. Fixed base32 alphabets need no temporary table. `custom` allocates its
alphabet and a 128-byte decoding table once; its shared methods do not mutate
them. Construction and all codec allocations report `EncodingError.Allocation`.
The source and existing encoding remain unchanged on failure.

MIME encoding holds ordinary base64 plus its wrapped result; MIME decoding
holds a whitespace-free copy plus its decoded result. These are whole-buffer
copies, not zero-copy adapters. `_text` additionally copies output into a `str`;
`_from_text` additionally copies input into `Bytes`, as with existing codecs.

All operations take linear time in input length. Base32 output is
`8 * ceil(n / 5)` bytes. Unpadded custom output is `ceil(8n / bits_per_symbol)`;
padded output rounds that to a whole symbol group. New radix encoders reject
unrepresentable size arithmetic with `InvalidLength`. No custom bit order,
case folding, ignored-character configuration, or arbitrary wrapping is exposed.
