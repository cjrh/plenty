# Pack bytes in a compact buffer

For a binary record, frame, or file body, `std.bytes.Bytes` provides checked
byte-range operations and works with byte-oriented I/O. Its contiguous buffer
stores one byte per element. A `list[u8]` also packs its elements into bytes;
use its ordinary list operations and element references when those suit the task.

```plenty
import std.bytes
from std.bytes import Bytes, BytesError

def checksum(view: &Bytes, start: i64, stop: i64) -> Result[i64, BytesError]:
    mut total = 0
    for index in range(start, stop):
        total += i64(view.get(index)?)
    Ok(total)

def main() -> Result[(), AllocError | IoError | BytesError]:
    mut frame = std.bytes.with_capacity(8)?
    frame.push(2u8)?
    frame.extend(&std.bytes.from_text("hi")?)?
    frame.push(0u8)?
    frame.push(255u8)?
    print(frame.len(), frame.capacity(), frame)?
    print(checksum(&frame, 1, 3)?)?
    print(str.repr(checksum(&frame, 3, 9))?)?
    payload = frame.slice(1, 3)?
    print(payload.to_text()?)?
    print(str.repr(frame.to_text())?)?
    Ok(())
```
```output
5 8 std.bytes.Bytes(_owner=b"\x02hi\x00\xff")
209
Result[i64, std.bytes.BytesError].Err(std.bytes.BytesError.OutOfRange)
hi
Result[str, std.bytes.BytesError].Err(std.bytes.BytesError.InvalidUtf8)
```

Every fallible operation returns `Result` with one error type, `BytesError`.
A range is `start` up to but not including `stop`, and must lie inside the
buffer: there are no negative offsets and nothing is clamped, so `3..9` on five
bytes is `OutOfRange`. `slice` and `copy` make independent buffers; nothing is
copied unless you ask. `to_text` succeeds only for valid UTF-8, and a zero byte
is ordinary data.

`checksum` shows how to work on part of a buffer: borrow it and pass the range.
While that borrow is in use the owner cannot change, so growth can never move
bytes out from under a reader:

```plenty-error
import std.bytes

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:
    mut frame = std.bytes.from_text("hi")?
    view = &frame
    frame.push(0u8)?
    print(view.len())?
    Ok(())
```

```error
conflicting borrow: cannot modify or exclusively borrow `frame` while it is borrowed
```

File, argument, and stream builtins still exchange `list[u8]`. Convert at the
edge with `from_list` and `to_list`; each is an explicit copy:

```plenty
import std.bytes

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:
    mut record = std.bytes.from_list(&encode_utf8("name=")?)?
    record.extend(&std.bytes.from_list(&[255u8, 0u8]?)?)?
    write_stdout_bytes(&record.slice(0, 5)?.to_list()?)?
    print(record.len(), record.find(61u8, 0, record.len())?)?
    Ok(())
```
```output
name=7 Option[i64].Some(4)
```

A parser should not call `get` once per byte: each call costs far more than
the comparison it feeds. The scans search a whole range in one call and return
an absolute index. `find_in` stops at any byte of a set, `find_not_in` at the
end of a run, and `matches_text` compares in place without copying.
`to_text_range` copies one range out as checked text, with no intermediate
`Bytes`:

```plenty
import std.bytes

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:
    line = std.bytes.from_text("name=Ada;  role=engineer")?
    stops = std.bytes.from_text("=;")?
    blank = std.bytes.from_text(" ")?
    mut at = 0
    while at < line.len():
        match line.find_in(&stops, at, line.len())?:
            case Some(index):
                print(index, line.to_text_range(at, index)?)?
                at = index + 1
            case Nothing:
                print("rest", line.to_text_range(at, line.len())?)?
                at = line.len()
    print(line.find_not_in(&blank, 9, line.len())?)?
    print(line.matches_text(11, "role")?, line.count(32u8, 0, line.len())?)?
    Ok(())
```
```output
4 name
8 Ada
15   role
rest engineer
Option[i64].Some(11)
True 2
```

Index syntax reads and writes single bytes, with the same rules as a list
index: a negative index counts from the end, and a missing one stops the
program with `index out of bounds`. Iterating over `&buffer` reads each byte
without consuming the buffer:

```plenty
import std.bytes

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:
    mut word = std.bytes.from_text("plenty")?
    word[0] = 80u8
    word[-1] -= 32u8
    print(word.to_text()?, word[1])?
    mut vowels = 0
    for b in &word:
        if b == 101u8 or b == 89u8:
            vowels += 1
    print(vowels)?
    word.fill(1, 5, 46u8)?
    print(word.to_text()?)?
    Ok(())
```
```output
PlentY 108
2
P....Y
```

Use `get` and `set` instead when a missing index is an expected outcome to
handle. `for b in &mut word` is rejected: write through an index. There is no
slice syntax; `slice` makes its copy explicit.

Fixed binary data, such as a file signature or a protocol delimiter, is
written as a byte literal: `b"..."` holds ASCII characters, the string escapes,
and `\xHH` for any other byte. Its type is `bytes`, an immutable value in the
program's read-only data, so it never allocates. It has a length, indexing, and
iteration:

```plenty
import std.bytes

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:
    magic = b"\x89PNG"
    print(magic, len(magic), magic[1])?
    mut total = 0
    for byte in magic:
        total += i64(byte)
    print(total)?
    Ok(())
```
```output
b"\x89PNG" 4 80
366
```

Operations that only read another sequence of bytes, such as `find_bytes`,
`matches`, `find_in`, and `extend`, take a literal as readily as a buffer. Their
parameter is generic over `ByteSource`, a protocol that both `bytes` and `Bytes`
implement, so each call reads the bytes where they are and nothing is copied.
Your own functions can accept either form the same way:

```plenty
import std.bytes
from std.bytes import ByteSource

def starts_png[S: ByteSource](data: &S) -> bool:
    data.len() >= 4 and data[0] == 137u8 and data[1] == 80u8

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:
    magic = b"\x89PNG"
    mut header = std.bytes.from_bytes(magic)?
    header.extend(b"\r\n\x1a\n")?
    print(header.find_bytes(b"\r\n", 0, header.len())?)?
    print(header.matches(0, &magic, 0, 4)?, header.find_in(b"\n\r", 0, header.len())?)?
    print(starts_png(&header), starts_png(&magic), starts_png(b"GIF8"))?
    Ok(())
```
```output
Option[i64].Some(4)
True Option[i64].Some(4)
True True False
```

The literal is never converted into a `Bytes` behind your back. To change bytes,
or to call a `Bytes` method such as `find` or `slice` on them, copy the literal
with `from_bytes`, as `header` does; the copy allocates, so it returns a
`Result`. Stream writes in `std.io` accept a literal the same way, as the next
lessons show.

A byte literal accepts only ASCII characters, so `b"é"` is an error; use
`from_text` for the UTF-8 encoding of text. See
[byte literals](../../design/language/03-types/bytes.md) for the full rules.

See [the module reference](../../design/stdlib/bytes.md) for every operation,
its costs, and what views cannot do yet.
