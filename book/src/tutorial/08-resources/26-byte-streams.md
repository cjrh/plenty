# Read and write streams in pieces

A file, a pipe, or a terminal rarely moves all the bytes you ask for in one
call. `std.io` makes that visible: one `read` or `write` reports how many bytes
it moved, and helpers repeat the call until a range is complete.

A stream is any `pub` class with the right methods. This one hands out its
text at most three bytes at a time, as a slow pipe would:

```plenty
import std.bytes
import std.io
from std.bytes import Bytes
from std.io import StreamError

pub class Trickle:
    pub data: Bytes
    pub at: i64

    pub def read(self: &mut Trickle, buffer: &mut Bytes, start: i64, stop: i64) -> Result[i64, StreamError]:
        mut count = stop - start
        if count > 3:
            count = 3
        if count > self.data.len() - self.at:
            count = self.data.len() - self.at
        match buffer.overwrite(start, &self.data, self.at, self.at + count):
            case Ok(_):
                self.at += count
                Ok(count)
            case Err(_):
                Err(StreamError.OutOfRange)

def main() -> Result[(), Failure]:
    mut source = Trickle(std.bytes.from_text("0123456789")?, 0)
    mut buffer = std.bytes.with_capacity(8)?
    buffer.resize(8, 46u8)?
    print(source.read(&mut buffer, 0, 8)?, buffer.to_text()?)?
    print(str.repr(std.io.read_exact(&mut source, &mut buffer, 0, 4))?, buffer.to_text()?)?
    print(str.repr(std.io.read_exact(&mut source, &mut buffer, 0, 8))?)?
    print(source.read(&mut buffer, 0, 8)?)?
    Ok(())
```
```output
3 012.....
Result[(), std.io.Partial].Ok(()) 3456....
Result[(), std.io.Partial].Err(std.io.Partial(completed=3, cause=std.io.StreamError.UnexpectedEof))
0
```

`read` fills part of `buffer[start:stop]` and returns the count. Three of eight
bytes is a short read, not an error; zero means end of input. `read_exact`
calls `read` until the range is full. When the input ends first, the error says
how many bytes arrived, and they are in the buffer.

Writing has the same shape. `write` may accept only part of a range, and
`write_all` continues until all of it is delivered. A `BufWriter` collects
small writes in a fixed-size buffer and passes them on in larger pieces:

```plenty
import std.bytes
import std.io

def main() -> Result[(), Failure]:
    mut input = std.io.buffered_reader(std.io.cursor(std.bytes.from_text("red\ngreen\nblue")?), 4)?
    mut output = std.io.buffered_writer(std.io.stdout(), 16)?
    mut line = std.bytes.empty()?
    mut count = 0
    while input.read_until(10u8, &mut line, 64)? > 0:
        count += 1
        std.io.write_all(&mut output, b"line: ", 0, 6)?
        std.io.write_all(&mut output, &line, 0, line.len())?
        line.clear()
    std.io.write_all(&mut output, b"\n", 0, 1)?
    output.flush()?
    print(count, output.pending(), output.capacity())?
    Ok(())
```
```output
line: red
line: green
line: blue
3 0 16
```

`write_all` takes the byte literal `b"line: "` as readily as the buffer
`line`: a stream's `write` accepts any `std.bytes.ByteSource` and reads it in
place. A class of your own becomes a writer with a `flush` method and a
`write` method generic in its source, `def write[S: ByteSource](...)` taking
`source: &S`; the [module reference](../../design/stdlib/io.md#stream-contracts)
shows one.

`cursor` turns a `Bytes` into a stream, which is how to test stream code
without a file. `read_until` appends one line, including its newline, and never
more than the limit of 64 bytes, so a file with no newline cannot exhaust
memory. Both buffers are allocated once, with the capacity you pass, and never
grow. Output reaches the terminal when the buffer fills and at `flush`, so
flush before printing anything that must come after it.

Bytes held by a `BufWriter` are not written when it goes out of scope. Call
`flush`, or `close_buffered` for a stream that also needs closing:

```plenty
import std.bytes
import std.io
from std.io import SeekFrom

def main() -> Result[(), Failure]:
    mut file = std.io.buffered_writer(std.io.open_file("streams-lesson.bin", "w+")?, 8)?
    record = std.bytes.from_list(&[1u8, 0u8, 255u8, 10u8, 7u8]?)?
    std.io.write_all(&mut file, &record, 0, 5)?
    print(file.pending(), std.io.seek_writer(&mut file, SeekFrom.Start(2))?, file.pending())?
    mut back = std.bytes.with_capacity(3)?
    back.resize(3, 0u8)?
    std.io.read_exact(&mut file.inner, &mut back, 0, 3)?
    print(back.to_list()?)?
    print(str.repr(std.io.close_buffered(&mut file))?)?
    print(str.repr(file.flush())?)?
    Ok(())
```
```output
5 2 0
[255, 10, 7]
Result[(), std.io.CloseError].Ok(())
Result[(), std.io.StreamError].Err(std.io.StreamError.Io(IoError.Closed))
```

Five bytes wait in the buffer until `seek_writer` delivers them and moves the
file to byte 2. File streams carry raw bytes: 255 and the newline byte come
back unchanged. `close_buffered` flushes, closes even when the flush fails, and
reports which of the two went wrong.

See [the module reference](../../design/stdlib/io.md) for every error, the
copy helper, and the standard stream adapters.
