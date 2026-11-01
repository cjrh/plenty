# Read and write binary packets

A [`Cursor`](../../design/stdlib/io-binary.md) keeps the byte position for a
packet. Its typed operations read or write one complete field, with explicit
byte order and no manual offset bookkeeping.

```plenty
import std.bytes
import std.io
from std.encoding import Endian
from std.io import SeekFrom

def main() -> Result[(), IoError | RangeError | std.bytes.BytesError | std.io.BinaryError | std.io.StreamError]:
    payload = b"hello"
    mut packet = std.io.cursor(std.bytes.empty()?)
    packet.write_u8(1u8)?
    # Check that a payload size fits its wire field before converting it.
    packet.write_u32(u32.try_from(len(payload))?, Endian.Big)?
    packet.write_f32(1.5f32, Endian.Big)?
    packet.write_bytes(payload, 64)?
    drop(packet.seek(SeekFrom.Start(0))?)

    version = packet.read_u8()?
    length = i64(packet.read_u32(Endian.Big)?)
    scale = packet.read_f32(Endian.Big)?
    # A hostile length is rejected before allocating its owned payload copy.
    body = packet.read_bytes(length, 64)?
    packet.require_end()?
    print(version, scale, body.to_text()?, packet.position())?
    Ok(())
```
```output
1 1.5 14
```

The checked `u32` conversion propagates `RangeError` if a payload length cannot
fit the wire field. `read_bytes` returns an owned copy because borrowed views
cannot escape a method. Scalar fields allocate nothing when storage already has capacity.

If a field is truncated, `BinaryError.Truncated` reports its position, requested
width, and available bytes. The failed operation leaves the cursor in place;
previous fields remain consumed. `require_end` detects unexpected trailing
bytes. Raw stream reads and writes and `seek` use the same position, so they
can be interleaved with typed fields when the format calls for them.
