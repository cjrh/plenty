# Own a foreign handle

Own a C resource with a private opaque pointer and a destructor. This interface
wraps a temporary C file, keeping raw operations private and reporting acquisition
failure through its public factory.

Save this as `c_scratch.plentyi`:

```plenty-file c_scratch.plentyi
opaque RawFile
extern def create_raw() -> RawFile = "tmpfile"
extern def close_raw(file: RawFile) -> i32 = "fclose"
extern def position_raw(file: RawFile) -> i64 = "ftell"

# fwrite borrows the bytes and file only for the call; it retains neither.
# The adapter expands to (data, byte_len), followed by count and file.
extern def write_raw(text: &str as utf8, count: u64, file: RawFile) -> u64 = "fwrite"

pub class Scratch:
    raw: RawFile
    def __init__(self) -> ():
        self.raw = RawFile.null()
    def __del__(self) -> ():
        if not self.raw.is_null():
            drop(close_raw(self.raw))
    pub def write(self: &mut Scratch, text: &str) -> Result[(), Failure]:
        if len(text) == 0:
            return Ok(())
        if write_raw(text, 1u64, self.raw) != 1u64:
            return Err(Failure.Unspecified)
        Ok(())
    pub def position(self: &mut Scratch) -> Result[i64, Failure]:
        value = position_raw(self.raw)
        if value < 0:
            return Err(Failure.Unspecified)
        Ok(value)

pub def create() -> Result[Scratch, Failure]:
    mut owner = Scratch()
    owner.raw = create_raw()
    if owner.raw.is_null():
        return Err(Failure.Unspecified)
    Ok(owner)
```

```plenty
import c_scratch

def main() -> Result[(), Failure]:
    mut file = c_scratch.create()?
    text = "é\0A"
    file.write(&text)?
    print(file.position()?)?
    Ok(())
```

```output
4
```

Initialize the owner to null before acquiring the resource, so cleanup is valid
even if acquisition fails. After success, the destructor closes the file through
its originating library, including on early exit. The owner cannot be copied.

The destructor discards close errors. Offer an explicit fallible `close` method
when callers need to handle them.
Native ownership transfer needs its own wrapper: clear the field when C consumes
the handle, and retain or return the owner when C leaves ownership with the caller.
