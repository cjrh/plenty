# Build a C library

Use `export def` to name the C symbol for a function. This `calc.plenty` module
can also be imported and called by an ordinary Plenty program:

```plenty-file calc.plenty
pub export def add(a: i32, b: i32) -> i32 = "calc_add":
    "Add two checked integers."
    a + b
```

```plenty
import calc

def main() -> Result[(), Failure]:
    print(calc.add(20, 22))?
    Ok(())
```
```output
42
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

For a static archive, use `--static-library` instead. Pass the native arguments
listed in `calc.link-args.txt` after the archive when linking a C program.

Exports currently support numeric scalars and unit returns. Read the generated
header's requirements even for simple APIs: runtime traps can terminate the
process, and current libraries require serialized calls on one caller thread.
Managed objects and ownership-transferring exports need further adapters. See the
[library reference](../design/24-c-interfaces/02-library-exports.md) for packaging,
metadata discovery, and exact limits.
