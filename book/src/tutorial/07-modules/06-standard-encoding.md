# Encode bytes and pack integers

Binary formats store integers at fixed widths in a stated byte order, and text
channels carry binary data as hex or base64. `std.encoding` does both over
[`std.bytes`](../08-resources/24-byte-buffers.md) buffers.

```plenty
import std.bytes
import std.encoding
from std.encoding import Endian

def main() -> Result[(), Failure]:
    mut frame = std.bytes.empty()?
    std.encoding.append_u16(&mut frame, 513u16, Endian.Big)?
    std.encoding.append_u32(&mut frame, 7u32, Endian.Little)?
    frame.extend(&std.bytes.from_text("hi")?)?
    print(std.encoding.hex_text(&frame, 0, frame.len())?)?
    print(std.encoding.base64_text(&frame, 0, frame.len())?)?
    print(std.encoding.base64url_text(&frame, 0, 7)?)?
    print(std.encoding.unpack_u16(&frame, 0, Endian.Big)?, std.encoding.unpack_u16(&frame, 0, Endian.Little)?)?
    std.encoding.pack_u32(&mut frame, 2, 8u32, Endian.Little)?
    print(std.encoding.unpack_u32(&frame, 2, Endian.Little)?, frame.len())?
    Ok(())
```
```output
0201070000006869
AgEHAAAAaGk=
AgEHAAAAaA
513 258
8 8
```

Every integer function names its width and takes the byte order as an argument;
there is no default order. `Endian.Big` stores the most significant byte first.
The same two bytes read as `513` or `258` depending on the order you state.
`append_` grows the buffer by the integer's width. `pack_` overwrites bytes that
already exist and never grows it. `unpack_` reads at an offset.

The codecs take a borrowed buffer and a `start`/`stop` range, as `std.bytes`
operations do. The `_text` forms return a `str`; `hex_encode`, `base64_encode`,
and `base64url_encode` return the encoded bytes as a new buffer. `base64` uses
`+` and `/` and always pads with `=`. `base64url` uses `-` and `_` and never
pads.

Decoding accepts exactly one form and reports what is wrong with anything else.
Nothing is skipped or repaired, including spaces and line breaks:

```plenty
import std.bytes
import std.encoding
from std.encoding import Endian

def main() -> Result[(), Failure]:
    print(std.encoding.base64_from_text("aGk=")?.to_text()?)?
    print(str.repr(std.encoding.hex_from_text("abc"))?)?
    print(str.repr(std.encoding.hex_from_text("0g"))?)?
    print(str.repr(std.encoding.base64_from_text("aGk"))?)?
    print(str.repr(std.encoding.base64_from_text("aG k"))?)?
    print(str.repr(std.encoding.base64_from_text("aGl="))?)?
    print(str.repr(std.encoding.base64url_from_text("aGk="))?)?
    short = std.bytes.from_text("abc")?
    print(str.repr(std.encoding.unpack_u32(&short, 0, Endian.Big))?)?
    Ok(())
```
```output
hi
Result[std.bytes.Bytes, std.encoding.EncodingError].Err(std.encoding.EncodingError.InvalidLength)
Result[std.bytes.Bytes, std.encoding.EncodingError].Err(std.encoding.EncodingError.InvalidByte(1))
Result[std.bytes.Bytes, std.encoding.EncodingError].Err(std.encoding.EncodingError.InvalidLength)
Result[std.bytes.Bytes, std.encoding.EncodingError].Err(std.encoding.EncodingError.InvalidByte(2))
Result[std.bytes.Bytes, std.encoding.EncodingError].Err(std.encoding.EncodingError.NonCanonical(2))
Result[std.bytes.Bytes, std.encoding.EncodingError].Err(std.encoding.EncodingError.InvalidPadding(3))
Result[u32, std.encoding.EncodingError].Err(std.encoding.EncodingError.OutOfRange)
```

The number in an error is the index of the offending byte. `aGl=` is
`NonCanonical` because its last symbol carries bits that belong to no byte:
`aGk=` is the only accepted text for `hi`. Reading four bytes from a
three-byte buffer is `OutOfRange`, never a partial value.

Decoded bytes are not text. Call `to_text()` on the result when you expect
UTF-8; it validates and can fail on its own.

See the [encoding reference](../../design/stdlib/encoding.md) for every
operation, its errors, and its costs.
