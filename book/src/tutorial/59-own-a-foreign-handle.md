# Own a foreign handle

Use a class with a private opaque pointer and `__del__` to own a C resource.
Application code gets ordinary Plenty moves, borrowing, and automatic cleanup.
This interface owns a temporary C file; its public factory reports acquisition
failure and keeps its raw declarations private.

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
    mut owner = Scratch()?
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

`Scratch()` allocates the Plenty owner before acquiring a C resource. Its null
field is fully initialized, so cleanup is active even if later work fails. Once
the factory returns, the resource moves into `file`. Leaving scope, including
through `?`, closes it exactly once. The library that creates the file also
destroys it. A class with a destructor cannot be copied.

This small example discards close errors in `__del__`; a production binding can
also offer an explicit fallible `close` method when callers need to handle them.
Native ownership transfer needs its own wrapper: clear the field when C consumes
the handle, and retain or return the owner when C leaves ownership with the caller.
