# Keep bytes separate from text

Use `list[u8]` when data may contain invalid UTF-8. Encoding text copies its exact
UTF-8 bytes; decoding validates every byte. Both operations observe their input,
so you can keep the original data.

```plenty
def main() -> Result[(), IoError]:
    text = "é\r\n"
    bytes = encode_utf8(text)?
    print(len(bytes))?
    print(decode_utf8(&bytes)? == text)?
    print(len(bytes))?
    invalid = encode_utf8("x")?
    mut raw = invalid
    raw[0] = 255u8
    match decode_utf8(&raw):
        case Ok(_):
            print("unexpected text")?
        case Err(error):
            print(error)?
    Ok(())
```
```output
4
True
4
IoError.Data(DataError.InvalidUtf8)
```

On Unix, filenames and executable arguments can contain arbitrary bytes except
NUL. Use `args_bytes()` to obtain those arguments, then pass a path byte list to
`read_bytes(path)` or `write_bytes(path, contents)`. These helpers preserve file
contents and names without text decoding or newline conversion. They currently
support Linux. `write_bytes` creates or replaces its destination.

For supplied paths on stdin, use `input_bytes(0u8)`. It returns one NUL-delimited
record as `Some(bytes)` or `Nothing` at EOF. Write records with
`write_stdout_bytes(bytes)` followed by `write_stdout_bytes(terminator)`, where
`terminator = encode_utf8("\0")?`. Finish with `flush_stdout()?`. Empty records
are distinct from EOF; a final nonempty unterminated record is accepted.

CR, LF, and invalid UTF-8 remain part of the path. Keep the original bytes when
opening files, even if you separately decode or format them for display. The
[finder experiment](../../design/examples/finder.md) includes a working `--null`
mode. Its globs match bytes in that mode, so `?` matches one byte.

See the [binary I/O reference](../../design/facilities/02-io/bytes-paths.md) for
partial effects, allocation failures, and remaining library limits.
