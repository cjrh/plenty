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

pub enum ScratchError:
    Create
    Write
    Position

pub class Scratch:
    raw: RawFile
    def __init__(self) -> ():
        self.raw = RawFile.null()
    def __del__(self) -> ():
        if not self.raw.is_null():
            drop(close_raw(self.raw))
    pub def write(self: &mut Scratch, text: &str) -> Result[(), ScratchError]:
        if len(text) == 0:
            return Ok(())
        if write_raw(text, 1u64, self.raw) != 1u64:
            return Err(ScratchError.Write)
        Ok(())
    pub def position(self: &mut Scratch) -> Result[i64, ScratchError]:
        value = position_raw(self.raw)
        if value < 0:
            return Err(ScratchError.Position)
        Ok(value)

pub def create() -> Result[Scratch, ScratchError]:
    mut owner = Scratch()
    owner.raw = create_raw()
    if owner.raw.is_null():
        return Err(ScratchError.Create)
    Ok(owner)
```

```plenty
import c_scratch

def main() -> Result[(), c_scratch.ScratchError | IoError]:
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

## Move the owner to a worker

Worker code can hold an opaque pointer only if its declaration states a thread
contract. `@thread_movable` promises that one thread at a time may use and
release the pointee, whichever thread that is. POSIX makes each stdio call
safe from any thread, so a temporary file qualifies. Every C function that
takes or returns a marked type must then be `@thread_safe`:

```plenty-file c_worker_scratch.plentyi
@thread_movable
opaque RawFile
@thread_safe
extern def create_raw() -> RawFile = "tmpfile"
@thread_safe
extern def close_raw(file: RawFile) -> i32 = "fclose"
@thread_safe
extern def position_raw(file: RawFile) -> i64 = "ftell"
@thread_safe
extern def write_raw(text: &str as utf8, count: u64, file: RawFile) -> u64 = "fwrite"

pub class Scratch:
    raw: RawFile
    def __init__(self) -> ():
        self.raw = RawFile.null()
    def __del__(self) -> ():
        if not self.raw.is_null():
            drop(close_raw(self.raw))
    pub def write(self: &mut Scratch, text: &str) -> i64:
        if len(text) > 0 and write_raw(text, 1u64, self.raw) != 1u64:
            return -1
        position_raw(self.raw)

pub def create() -> Option[Scratch]:
    mut owner = Scratch()
    owner.raw = create_raw()
    if owner.raw.is_null():
        return Nothing
    Some(owner)
```

```plenty
import c_worker_scratch
from c_worker_scratch import Scratch

# The worker owns the file, so its destructor closes it on that thread.
def fill(file: Scratch) -> i64:
    mut owned = file
    owned.write("worker")

def main() -> Result[(), IoError | SpawnError[()]]:
    match c_worker_scratch.create():
        case Some(file):
            with spawn(fill, file).drop_job()? as task:
                print(task.join())?
        case Nothing:
            print("no temporary file")?
    Ok(())
```

```output
6
```

A movable pointer crosses only inside an owner like `Scratch`: a class that
holds the only copy and has a `__del__`, so `copy(x)` rejects it. Passing the bare `RawFile`, borrowing the
owner or putting it in `Shared` is rejected, because a copy left behind would
let two threads use one stream. Mark a type `@thread_shareable` only when any
number of threads may use one pointee at once and no declared call writes
through it. See
[opaque types on worker threads](../../design/facilities/04-interop/trusted-declarations.md#opaque-types-on-worker-threads)
for the rules and when a binding may promise each contract.
[How std.dataframe wraps its engine](../../design/facilities/04-interop/dataframe-binding.md)
shows a library that builds values with movable builders and then shares them.

## Host-supplied callback tables

For a library called by a native host, keep protocol-specific pointer operations
in a trusted interface. Mark its indirect native declarations `@host_callback`
only when the host guarantees the current callback context, exact ABI, live
addresses and synchronized data. A `@thread_safe` Plenty export can then call
those declarations. The annotation does not authorize those operations on
Plenty-created workers, even through an export or a helper.

This is a trusted binding contract, not automatic C record layout or a real-time
guarantee. See [host protocol callbacks](../../design/facilities/04-interop/host-threads.md#host-protocol-callbacks)
for the declaration and full obligations.
