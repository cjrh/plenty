# Target conditionals

`if const` selects code by facts about the compilation target. It may appear
at module scope, around declarations and imports, and in any function body,
around statements:

```plenty
if const target.os == "windows":
    import windows_console
    def separator() -> str:
        return "\\"
else:
    def separator() -> str:
        return "/"

def main() -> Result[(), IoError]:
    if const target.pointer_width == 64:
        limit = 1 << 40
    else:
        limit = 1 << 20
    print(separator())?
    print(limit)?
    Ok(())
```

On a 64-bit Linux or macOS target, it prints:

```output
/
1099511627776
```

The compiler decides the condition while parsing and keeps only the first arm
whose condition holds, or the `else` arm. Each continuation is written
`elif const`; an `elif` without `const` is an error. Unselected arms must
parse, but are not resolved or checked: they may import modules, call
functions, and name declarations that exist only on another target. An
unselected arm is not checked on the current target, so build on each target a
program supports.

In a function body, the selected statements take the conditional's place in
the enclosing block; they open no new scope. A binding in the selected arm,
such as `limit` above, is visible after the conditional. The last statement
of a selected arm can be the function's result.

## Facts

| Fact | Known values |
|---|---|
| `target.os` | `"linux"`, `"macos"`, `"windows"`, `"freebsd"` |
| `target.arch` | `"x86_64"`, `"aarch64"` |
| `target.pointer_width` | `32`, `64` |
| `target.endian` | `"little"`, `"big"` |

The set is closed. An unknown fact, such as `target.cpu`, is an error, and so is
a comparison with a value outside the known values, such as `"linx"`, so a
misspelling cannot silently select the `else` arm. The compiler currently
builds natively for GNU/Linux on x86-64 and AArch64: `target.os` is `"linux"`,
`target.arch` is `"x86_64"` or `"aarch64"`, pointer width is `64`, and endian is
`"little"`. The facts describe the packaged runtime, and an installed compiler
cannot select the other architecture with `--target`; see [platform support](../../toolchain/02-architecture/support.md).

## Conditions

A condition compares one fact with a string or integer literal using `==` or
`!=`, and combines comparisons with `and`, `or`, `not`, parentheses, `True`,
and `False`. Any other expression is an error. A condition cannot name a
[module constant](modules.md#constants): conditions are decided while parsing,
before names are resolved, so that unselected arms can name declarations that
do not exist. A constant can instead be defined differently in each arm.
