# Compress bytes and streams

`std.compress` reads and writes gzip, zlib and raw DEFLATE data. Whole
buffers go through `compress` and `decompress`; streams go through a
`Compressor` writer and a `Decompressor` reader from
[`std.io`](../../design/stdlib/io.md).

## Whole buffers

`compress` takes a format, the data, and a level from 0 (fastest) to 9
(smallest). `decompress` takes a limit on the output size and fails rather
than produce more:

```plenty
import std.bytes
import std.compress
from std.compress import Format

def main() -> Result[(), Failure]:
    mut text = std.bytes.empty()?
    for _ in range(0, 100):
        text.extend(&std.bytes.from_text("to be or not to be ")?)?
    packed = std.compress.compress(Format.Gzip, &text, 6)?
    print(text.len())?
    print(packed.len())?
    plain = std.compress.decompress(Format.Gzip, &packed, 10000)?
    print(plain == text)?
    print(str.repr(std.compress.decompress(Format.Gzip, &packed, 1000))?)?
    Ok(())
```
```output
1900
59
True
Result[std.bytes.Bytes, std.compress.CompressError].Err(std.compress.CompressError.LimitExceeded)
```

Repetitive text shrinks a lot. That is also the danger in decompressing:
a small input can expand a thousandfold. Always choose the limit from the
memory the program can spend, never from the size of the input.

## Streams

A `Compressor` wraps any writer and is itself a writer, so `write_all` and
`copy_all` work with it. `finish` writes the end of the stream; without it
the output is incomplete. A `Decompressor` wraps any reader and is itself a
reader, decoding as it goes in fixed-size buffers:

```plenty
import std.bytes
import std.compress
import std.io
from std.compress import Format

def main() -> Result[(), Failure]:
    line = std.bytes.from_text("one line of a log\n")?
    mut writer = std.compress.compressor(Format.Gzip, std.io.cursor(std.bytes.empty()?), 6)?
    for _ in range(0, 1000):
        std.io.write_all(&mut writer, &line, 0, line.len())?
    writer.finish()?
    packed = writer.inner.data.copy()?
    print(packed.len())?
    mut reader = std.compress.decompressor(Format.Gzip, std.io.cursor(packed), 1000000)?
    mut buffer = std.bytes.with_capacity(4096)?
    buffer.resize(4096, 0u8)?
    mut lines = 0
    while True:
        count = reader.read(&mut buffer, 0, 4096)?
        if count == 0:
            break
        lines += buffer.count(10u8, 0, count)?
    print(lines)?
    Ok(())
```
```output
185
1000
```

The reader counts 18,000 bytes of lines while holding at most 4096 of them,
so the same loop handles a file much larger than memory. To compress a file,
wrap `std.io.open_file(path, "w")?` instead of a cursor.

## Damaged input

gzip and zlib end with a checksum of the original data, so a damaged stream
is caught. `decompress` returns only an error. A `Decompressor` reports it
through the `Reader` protocol as a `StreamError`, and `failure` names the
exact cause:

```plenty
import std.bytes
import std.compress
import std.io
from std.compress import Format

def main() -> Result[(), Failure]:
    data = std.bytes.from_text("checked at the end")?
    mut packed = std.compress.compress(Format.Gzip, &data, 6)?
    last = packed.len() - 1
    packed.set(last, packed.get(last)? + 1u8)?
    print(str.repr(std.compress.decompress(Format.Gzip, &packed, 100))?)?
    mut reader = std.compress.decompressor(Format.Gzip, std.io.cursor(packed), 100)?
    mut out = std.bytes.empty()?
    match std.io.read_up_to(&mut reader, &mut out, 100):
        case Ok(_):
            print("read it all")?
        case Err(partial):
            print(str.repr(partial.cause)?)?
            print(str.repr(reader.failure())?)?
    Ok(())
```
```output
Result[std.bytes.Bytes, std.compress.CompressError].Err(std.compress.CompressError.Checksum)
std.io.StreamError.Io(IoError.InvalidInput)
Option[std.compress.CompressError].Some(std.compress.CompressError.Checksum)
```

The checksum comes last, so a reader may already have returned some bytes
before it finds the damage. Treat streamed output as unverified until `read`
returns 0.

See the [`std.compress` reference](../../design/stdlib/compress.md) for every
error, the rules for input after a stream, and memory costs.
