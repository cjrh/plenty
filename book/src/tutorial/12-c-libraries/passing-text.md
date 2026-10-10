# Pass text to C

Choose the text adapter to match C's signature. `as utf8` lends a pointer and
byte length without allocating. `as c_string` makes a terminated copy and reports
embedded NULs or allocation failure through `CStrError`.

Save this interface as `c_text.plentyi`:

```plenty-file c_text.plentyi
# strlen reads only during the call and does not retain or return its input.
# C signature: size_t strlen(const char *text).
pub extern def byte_length(text: &str as c_string) -> Result[u64, CStrError] = "strlen"
```

```plenty
import c_text

def main() -> Result[(), Failure]:
    text = "é"
    print(c_text.byte_length(&text)?)?
    with_nul = "a\0b"
    match c_text.byte_length(&with_nul):
        case Ok(size):
            print(size)?
        case Err(error):
            print(error)?
    print(text)?
    Ok(())
```

```output
2
CStrError.EmbeddedNul
é
```

The C call returns a plain `size_t`, which is `u64` on this target. The generated
adapter returns `Result[u64, CStrError]` because preparing the argument may fail.
The temporary buffer is freed after the call; the original text is unchanged.
`CStrError.Allocation(error)` reports a failed buffer allocation. These conversion
errors do not replace any error status that the C function itself returns.

An interface for `uint64_t checksum(const uint8_t *data, size_t size)` would
instead declare `text: &str as utf8`. It receives all bytes, including embedded
NULs, and needs no temporary copy. Neither adapter lets C retain or modify the
buffer. Use a binding with an explicit ownership contract for those cases.
