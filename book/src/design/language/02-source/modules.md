# Modules and imports

`std` is a reserved toolchain namespace. The embedded modules
[std.math](../../stdlib/math.md), [std.path](../../stdlib/path.md)
and [std.fs](../../stdlib/fs.md) are available without local
sources. Unknown `std` modules never
fall back to the source root. Local files/directories and declarations named
`std`, and imports binding a non-standard module as `std`, are rejected.

```plenty fragment
import package.module
import package.module as module_alias
from package.module import Name as Alias
```

Comma-separated imports are supported. Imports occur only at module scope and
execute no initializers. Relative imports, wildcards, re-exports, and implicit
transitive imports are unsupported. Directories are namespaces, without
`__init__` execution.

The source root defaults to the entry file's canonical directory; `--module-root
DIR` selects another root. In a [package](../../toolchain/01-building/packages.md),
the root is `src/`, and the first component of an import may instead be a
declared dependency alias, which names that dependency's library; an alias may
not match a local top-level module. `a.b` resolves to `<root>/a/b.plenty` or the trusted
interface `<root>/a/b.plentyi`. Having both is ambiguous. Imported canonical
filenames and directories must have identifier components and cannot escape
the root. File/directory name collisions and import cycles are errors; a cycle
diagnostic names the actual dependency chain.

Canonical paths identify modules, so imports through different aliases or diamond
dependencies preserve the same nominal types. Private declarations remain private
to their defining module, regardless of directory ancestry. All reachable modules
are checked before execution. Imported modules need no entrypoint, and their
`main` functions are ordinary functions.

The [concurrency standard modules](../../stdlib/index.md) export existing builtin
identities. An explicit import such as `from std.thread import spawn` may bind
the matching builtin name; an import cannot replace a builtin with a different
symbol. Item and module aliases retain the same types and compiler safety checks.

## Constants

`const name: T = value` declares an immutable value at module scope;
`pub const` exports it. `T` must be a concrete integer or float type, `bool`,
`str`, or `bytes`. The compiler evaluates the initializer, a constant
expression, and each read uses the result:

```plenty
const KIB: i64 = 1 << 10
const MIB: i64 = KIB * KIB
const MASK: u8 = ~(1u8 << 3)
const RATIO: f32 = f32(MIB) / 3.0
const NAME: str = "plenty" + "-" + "lang"
const WIDE: bool = MIB > KIB and not (KIB == 0)

def main() -> Result[(), IoError]:
    print(MIB)?
    print(MASK)?
    print(RATIO)?
    print(NAME)?
    print(WIDE)?
    Ok(())
```
```output
1048576
247
349525.34
plenty-lang
True
```

A constant expression contains literals, other constants (from this module or
imported, in any declaration order), numeric type constants such as `u8.MAX` or
`f64.PI`, numeric casts such as `f32(x)` and `u8(x)`, parentheses, the unary
operators `-`, `~`, and `not`, the arithmetic, bitwise, shift, comparison, and
logical operators, `x if c else y`, and `+` on two `str` values. Calls, f-strings,
collections, and other expressions are rejected with "not a constant expression".

Evaluation follows the [numeric rules](../03-types/numbers.md) of the same
expression at run time, so the folded value is the value the program would
compute: `//` floors, `%` takes the divisor's sign, `/` divides floats only,
`<<` discards shifted-out bits, `>>` is arithmetic for signed types, and
operands must share a type. Unsuffixed literals take the type the context
expects. Where run time would trap, evaluation is a compile error instead:
overflow, division by zero, a shift count outside `0..width`, and an out-of-range
float-to-integer cast. As at run time, `x if c else y` evaluates only the
selected branch, and `and` and `or` evaluate their right operand only when it
decides the result; the other operand is still type-checked, so
`1 if D == 0 else 100 // D` is valid when `D` is zero. A cycle
is reported with its path, for example `constant A depends on itself: A -> B -> A`.

### Static assertions

`assert CONDITION` or `assert CONDITION, "message"` at module scope checks a
constant expression during compilation. A false condition fails the build with
`static assertion failed`, followed by the message when given. The message is
itself a constant `str` expression. An assertion declares no name and produces
no code.

```plenty
const PAGE: i64 = 4096
assert PAGE % 64 == 0, "pages hold whole cache lines"

def main() -> Result[(), IoError]:
    print(PAGE // 64)?
    Ok(())
```
```output
64
```

Module-scope `assert` uses the spelling of a Python `assert` statement, which
the compiler checks once instead of at run time. Plenty has no run-time
`assert` statement; inside a function, `assert` is an ordinary name.

### Included files

`include_str("path")` initializes a `str` constant and `include_bytes("path")`
a `bytes` constant with a file's contents:

```text
const SCHEMA: str = include_str("sql/schema.sql")
const ICON: bytes = include_bytes("assets/icon.png")
```

The argument is one string literal: a path relative to the directory of the
source file that contains it. Absolute paths and paths that resolve outside the
package's source root, including through `..` or symbolic links, are errors.
`include_str` rejects a file that is not valid UTF-8 and names the first
invalid byte. Both forms initialize a `const` only and may appear in other
constant expressions, for example `"> " + SCHEMA`. They require a source file,
so source passed without a path, such as an editor buffer, cannot use them.

The contents become read-only data in the executable; reads allocate nothing.
An included file is an input of the [program cache](../../toolchain/02-architecture/cache.md#program-objects):
editing it rebuilds the program.

### Binding and representation

Imports and aliases bind constants as values: `import std.math as math` permits
`math.pi`, while `from std.math import pi` permits `pi`. Constants are not
callables or mutable places. Private constants follow the same visibility
rules as private functions. A local binding may shadow an imported constant.
Constant names share the declaration namespace with functions and types.

Constants require no module initialization or allocation. Each read of a
numeric or `bool` constant compiles to an immediate of the constant's declared
type, and each read of a `str` or `bytes` constant to a reference to read-only
data, in debug and release builds; there is no getter function, global
storage, or code run at import time. See
[call-site substitution](../../toolchain/02-architecture/emission.md#call-site-substitution).
