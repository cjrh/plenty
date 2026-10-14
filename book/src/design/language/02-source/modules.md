# Modules and imports

`std` is a reserved toolchain namespace. The embedded modules
[std.math](../../stdlib/math.md), [std.path](../../stdlib/path.md)
and [std.fs](../../stdlib/fs.md) are available without local
sources. Unknown `std` modules never
fall back to the source root. Local files/directories and declarations named
`std`, and imports binding a non-standard module as `std`, are rejected.

```python
import package.module
import package.module as module_alias
from package.module import Name as Alias
```

Comma-separated imports are supported. Imports occur only at module scope and
execute no initializers. Relative imports, wildcards, re-exports, and implicit
transitive imports are unsupported. Directories are namespaces, without
`__init__` execution.

The source root defaults to the entry file's canonical directory; `--module-root
DIR` selects another root. `a.b` resolves to `<root>/a/b.plenty` or the trusted
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

## Numeric constants

`const name: T = value` declares an immutable numeric value at module scope;
`pub const` exports it. `T` must be a concrete integer or float type. The
initializer must be a numeric literal (optionally negative or parenthesized)
or a numeric type constant such as `f64.PI` or `u8.MAX`; float type constants
may also be negated. Ordinary type and range checks apply. Calls, arithmetic,
references to other module constants, type aliases, and nonnumeric values are
not supported as constant initializers.

Imports and aliases bind constants as values: `import std.math as math` permits
`math.pi`, while `from std.math import pi` permits `pi`. Constants are not
callables or mutable places. Private constants follow the same visibility
rules as private functions. A local binding may shadow an imported constant.
Constant names share the declaration namespace with functions and types.

Constants require no module initialization or allocation. The compiler lowers
each read through a private scalar getter; no mutable global storage or user
code runs at import time.
