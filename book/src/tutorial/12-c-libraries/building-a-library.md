# Build a C library

Use `export def` to name the C symbol for a function. This `calc.plenty` module
can also be imported and called by an ordinary Plenty program:

```plenty-file calc.plenty
pub enum CalcError:
    Overflow

pub export def add(a: i32, b: i32) -> Result[i32, CalcError] = "calc_add":
    "Add two checked integers."
    result = checked:
        a + b
    result.map_error(def(error: OverflowError[i32]) -> CalcError: CalcError.Overflow)

pub export def increment(value: &mut i32) -> Result[(), CalcError] = "calc_increment":
    result = checked:
        *value + 1
    *value = result.map_error(def(error: OverflowError[i32]) -> CalcError: CalcError.Overflow)?
    Ok(())
```

```plenty
import calc

def main() -> Result[(), IoError | calc.CalcError]:
    print(calc.add(20, 22)?)?
    mut value = 10i32
    calc.increment(&mut value)?
    print(value)?
    mut maximum = i32.MAX
    match calc.increment(&mut maximum):
        case Err(calc.CalcError.Overflow):
            print("overflow", maximum == i32.MAX)?
        case Ok(_):
            print("unexpected success")?
    Ok(())
```
```output
42
11
overflow True
```

Arithmetic errors are mapped locally to a payload-free error enum supported
by the C ABI. `increment` computes before assigning, so overflow leaves its
borrowed value unchanged. Builtin `OverflowError[T]` carries a payload and
cannot be exported directly. See [exported error contracts](export-result-contracts.md).

`pub` makes the function accessible through source imports. `export` independently
creates a C entry point in a library build. Libraries do not need `main`.

Save the module as `calc.plenty`, create a `build` directory, and run:

```sh
plenty --shared-library calc.plenty --library-name calc -o build/libcalc.so
```

The output includes `calc.h`, with a status-returning `calc_add` declaration
and generated contract comments, and `calc.plentyi`, which Plenty consumers can import. Copy that generated
interface into the consuming project's source root and link `libcalc.so` using
`--link-arg`; it replaces the source module there. C consumers include `calc.h`
and link the library.

If you only have the binary, recover its Plenty interface without loading it:

```sh
plenty --extract-interface build/libcalc.so --library-name calc -o calc.plentyi
plenty --verify-interface build/libcalc.so calc.plentyi
```

For a static archive, use `--static-library` instead. Pass the native arguments
listed in `calc.link-args.txt` after the archive when linking a C program.

For C callers, `increment` takes an `int32_t *` to an initialized value, borrowed
exclusively for the call. Plenty callers get this borrowing contract from the
generated interface.

Exports support numeric scalars, `bool`, borrows, unit, supported `Result` values, and
[owned class handles](export-owned-objects.md). Read the generated
header's requirements even for simple APIs: runtime traps can terminate the
process, and current libraries require serialized calls on one caller thread.
See the [library reference](../../design/facilities/04-interop/exports.md)
for supported export types and packaging details.

Boolean exports map directly to C `_Bool` and C++ `bool`:

```plenty
export def invert(value: bool) -> bool = "flags_invert":
    not value

def main() -> Result[(), IoError]:
    print(invert(False))?
    Ok(())
```
```output
True
```

The generated header includes `<stdbool.h>` and declares `bool flags_invert(bool p0)`.
Boolean borrows and `Result[bool, E]` success values also cross the C boundary.

Some hosts look up a data descriptor instead of calling an entry function.
Use a trusted [initialized data export](../../design/facilities/04-interop/exported-objects.md)
for that ABI. Its fields are ready before any Plenty function runs; filling
writable storage from a function would not satisfy such a discovery contract.
Keep the explicit native layout in the binding module and ordinary application
logic in Plenty functions.

For example, this binding exposes an immutable version field at byte offset zero:

```plenty-file descriptor.plentyi
pub opaque Pointer
pub extern def entry() -> Pointer = object "example_descriptor"(size=8, align=8):
    0: u32 = 1
pub extern def version(p: Pointer, offset: u64) -> u32 = intrinsic "load"
```

```plenty
import descriptor

def main() -> Result[(), IoError]:
    print(descriptor.version(descriptor.entry(), 0u64))?
```
```output
1
```

The eight-byte object includes four zero padding bytes. The opaque accessor
borrows its address for the library's loaded lifetime; it does not allocate an
owner. A real binding must match the host's native declaration and keep these
raw operations behind a documented interface.
