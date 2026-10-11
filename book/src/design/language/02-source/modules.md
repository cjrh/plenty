# Modules and imports

`std` is a reserved toolchain namespace. [The embedded math library](../../facilities/math.md)
is available as `std.math` without local sources. Unknown `std` modules never
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
