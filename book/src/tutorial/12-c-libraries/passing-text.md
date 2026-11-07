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

def main() -> Result[(), AllocError | IoError | CStrError]:
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

For a C string array, declare `arguments: &list[str] as c_strings` and return
`Result[T, CStrError]`. This builds a terminated pointer table and terminated
UTF-8 strings for one call; even the empty list has a final null pointer. C must
not modify or retain the table or its strings. See
[C string vectors](../../design/facilities/04-interop/borrowing-text.md#c-string-vectors).

When a native API reports failure through its return value and `errno`, add an
`error: &mut i32 as errno` parameter to its trusted declaration. This parameter
is omitted from C arguments and receives the calling thread's error immediately
after native return, before temporary buffers are freed. Check the native return
first, and inspect the captured value only if that API says it is meaningful.
Successful calls need not clear it. Argument-conversion failure leaves the slot
unchanged. See [native error capture](../../design/facilities/04-interop/borrowing-text.md#capture-native-errno).
