# Reading lines

`readline()` keeps the terminating newline, translating LF, CRLF, and bare CR
to `"\n"`. An empty line is `"\n"`; EOF is `""`. The last line need not have a
terminating newline. Unlike `input()`, line reading retains that terminator.

Run this in a scratch directory: it replaces `plenty-lines.txt`.

```plenty
def count_lines() -> Result[i64, IoError]:
    write_text("plenty-lines.txt", "first\r\n\rfinal")?
    mut count = 0
    with open("plenty-lines.txt")? as file:
        while True:
            line = file.readline()?
            if line == "":
                break
            count = count + 1
    Ok(count)

def main() -> Result[(), Failure]:
    print(count_lines()?)?
    Ok(())
```
```output
3
```

Each call reports decoding, I/O, or allocation failure through its Result. You
can mix `readline()` and `read()` on the same file; both share newline state.
Failures may consume input, so retrying is not a rollback. Direct `for line in
file` iteration is not yet implemented. Size-limited reads and saved positions are
introduced below.
