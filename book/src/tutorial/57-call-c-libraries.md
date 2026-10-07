# Call C libraries

Put trusted C declarations in a `.plentyi` interface, and import it like any
other module. This example calls the C standard library's integer absolute-value
function. Its C `int` parameters/results are `i32` on Plenty's supported target.
We wrap the raw operation so the minimum integer, which C cannot negate, is
reported as `Nothing`.

Save this as `c_math.plentyi`:

```plenty-file c_math.plentyi
extern def abs_raw(value: i32) -> i32 = "abs"

pub def absolute(value: i32) -> Option[i32]:
    if value == -2147483648i32:
        return Nothing
    Some(abs_raw(value))
```

```plenty
import c_math

def main() -> Result[(), Failure]:
    print(c_math.absolute(-42i32))?
    print(c_math.absolute(-2147483648i32))?
    Ok(())
```

```output
Option[i32].Some(42)
Option[i32].Nothing
```

The C standard library is already linked. For another library, pass its archive
or driver options explicitly, for example `--link-arg /path/to/libexample.a` or
`--link-arg -L/path/to/libs --link-arg -lexample`. A shared library also needs to
be discoverable by the system loader when the application runs.

An interface is a trust boundary. Its author checks the C header and documents
pointer lifetimes, ownership, and errors; application code should use its typed
wrappers. Plenty does not translate headers or verify the native implementation.
`pub` exposes a name to Plenty imports; it does not export a C symbol. See the
[C interface reference](../design/24-c-interfaces.md) for the supported ABI.
