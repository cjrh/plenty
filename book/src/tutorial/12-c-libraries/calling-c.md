# Call C libraries

Declare C functions in a `.plentyi` interface and import it as a module. C's
`abs` uses `int`, which is `i32` on Plenty's supported target. The wrapper rejects
the minimum integer, which C cannot negate.

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

The interface author must check the C header and document pointer lifetimes,
ownership, and errors. Plenty does not verify the native implementation;
application code should use typed wrappers.
`pub` exposes a name to Plenty imports; it does not export a C symbol. See the
[C interface reference](../../design/24-c-interfaces.md) for the supported ABI.
