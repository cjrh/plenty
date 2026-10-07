# Build a C library

Exports can return a numeric error through `Result`. Plenty callers use the
ordinary result; C callers receive a status and separate output pointers:

```plenty
export def positive(value: i32) -> Result[i32, i32] = "calc_positive":
    if value < 0:
        Err(1)
    else:
        Ok(value)

def main() -> Result[(), Failure]:
    print(positive(42)?)?
    print(positive(-1))?
    Ok(())
```
```output
42
Result[i32, i32].Err(1)
```

The generated C declaration is `uint32_t calc_positive(int32_t p0,
int32_t *out_ok, int32_t *out_error)`. Status 0 writes `out_ok`; status 1 writes
`out_error`. Its header explains output storage and borrowing requirements.

Use `export def` to name the C symbol for a function. This `calc.plenty` module
can also be imported and called by an ordinary Plenty program:

```plenty-file calc.plenty
pub export def add(a: i32, b: i32) -> i32 = "calc_add":
    "Add two checked integers."
    a + b

pub export def increment(value: &mut i32) -> () = "calc_increment":
    *value = *value + 1
```

```plenty
import calc

def main() -> Result[(), Failure]:
    print(calc.add(20, 22))?
    mut value = 10i32
    calc.increment(&mut value)
    print(value)?
    Ok(())
```
```output
42
11
```

`pub` makes the function accessible through source imports. `export` independently
creates a C entry point in a library build. Libraries do not need `main`.

Save the module as `calc.plenty`, create a `build` directory, and run:

```sh
plenty --shared-library calc.plenty --library-name calc -o build/libcalc.so
```

The output includes `calc.h`, with a `calc_add` declaration and generated contract
comments, and `calc.plentyi`, which Plenty consumers can import. Copy that generated
interface into the consuming project's source root and link `libcalc.so` using
`--link-arg`; it replaces the source module there. The same metadata is embedded
inside the library. Ordinary C consumers include `calc.h` and link the library.

If you only have the binary, recover its Plenty interface without loading it:

```sh
plenty --extract-interface build/libcalc.so --library-name calc -o calc.plentyi
```

For a static archive, use `--static-library` instead. Pass the native arguments
listed in `calc.link-args.txt` after the archive when linking a C program.

For C callers, `increment` takes an `int32_t *`. The generated comments explain
that it must point to an initialized value and be borrowed exclusively for the
call; the final value is written back before returning. Plenty callers get the
same borrowing rules automatically from the generated interface.

Exports support numeric scalars, borrows, unit, supported `Result` values, and
[owned class handles](61-export-owned-objects.md). Read the generated
header's requirements even for simple APIs: runtime traps can terminate the
process, and current libraries require serialized calls on one caller thread.
Other managed types need further adapters. See the
[library reference](../design/24-c-interfaces/02-library-exports.md) for packaging,
metadata discovery, and exact limits.
