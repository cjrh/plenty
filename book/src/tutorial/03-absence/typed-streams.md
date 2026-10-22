# Keep stream errors and progress

Use `std.io.typed` when a transfer needs the original cause and the number of
bytes delivered before failure. Its copy error distinguishes reading from
writing. Existing `std.io` streams can enter this API through bridges:

```plenty
import std.http.typed

import std.bytes
import std.io
import std.io.typed
from std.http.typed import fixed_body
from std.io.typed import CopyFailure, copy_all

def main() -> Result[(), AllocError | IoError | std.http.typed.FramingError[std.io.StreamError] | std.bytes.BytesError]:
    mut bytes = std.bytes.with_capacity(3)?
    bytes.resize(3, 65u8)?
    mut scratch = std.bytes.with_capacity(2)?
    scratch.resize(2, 0u8)?
    mut body = fixed_body(std.io.typed.reader(std.io.cursor(bytes)), 3)?
    mut output = std.io.typed.writer(std.io.cursor(std.bytes.with_capacity(3)?))
    match copy_all(&mut body, &mut output, &mut scratch):
        case Ok(count):
            print(count)?
        case Err(CopyFailure.Read(count, cause)):
            print("read failed after")?
            print(count)?
            drop(cause)
        case Err(CopyFailure.Write(count, cause)):
            print("write failed after")?
            print(count)?
            drop(cause)
    cursor = std.io.typed.unwrap_writer(output)
    print(cursor.position())?
    Ok(())
```

```output
3
3
```

The fixed body assumes an already validated Content-Length; it does not parse
HTTP. `close_body` instead finishes at EOF and requires a connection that will
never be reused. The copy helper retries explicit interruption and preserves
other transport causes. A write failure can leave unwritten data from the last
chunk in scratch; the delivered count alone is not a resume token.

For an already validated head with chunked transfer coding, use `chunked_body`
with explicit limits. Its typed errors include framing failures as well as the
transport cause. This small in-memory example propagates either kind of failure:

```plenty
import std.bytes
import std.http
import std.io
import std.io.typed
from std.http.typed import FramingError, chunked_body
from std.io.typed import CopyFailure, copy_all

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | FramingError[std.io.StreamError] | CopyFailure[FramingError[std.io.StreamError], std.io.StreamError]]:
    wire = std.bytes.from_text("3\r\nABC\r\n0\r\nX-Tag: sample\r\n\r\n")?
    limits = std.http.Limits(64, 128, 4, 1024)
    mut body = chunked_body(std.io.typed.reader(std.io.cursor(wire)), &limits)?
    mut output = std.io.typed.writer(std.io.cursor(std.bytes.with_capacity(3)?))
    mut scratch = std.bytes.with_capacity(2)?
    scratch.resize(2, 0u8)?
    print(copy_all(&mut body, &mut output, &mut scratch)?)?
    print(body.is_done(), body.trailers[0].value)?
    Ok(())
```

```output
3
True sample
```

Only decoded payload contributes to the copy count. The decoder keeps partial
framing state across interruption and stops after the trailers; it does not
parse or validate an HTTP head. Real clients should continue using
[`std.http.client`](../../design/stdlib/http-client.md), which also handles
head validation, connection ownership and reuse.

See the [typed streams reference](../../design/stdlib/io-typed.md) for nested
error payloads and the [framing reference](../../design/stdlib/http-typed.md)
for terminal-state rules.
