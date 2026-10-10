# Split a program into modules

An import names a source file. It does not execute that file. A library module
contains declarations and imports; only the application's `main` starts the
program. Names are private to their defining module unless marked `pub`.

Save this companion file as `geometry.plenty`:

```plenty-file geometry.plenty
pub class Point:
    pub x: i64
    pub y: i64

    pub def squared_length(self) -> i64:
        self.x * self.x + self.y * self.y

pub def make_point() -> Point:
    Point(3, 4)
```

Save the application beside it, for example as `main.plenty`:

```plenty
import geometry
from geometry import Point as Position

def main() -> Result[(), Failure]:
    point: Position = geometry.make_point()
    print(point.squared_length())?
    print(point.x)?
    Ok(())
```

```output
25
3
```

`import geometry` makes declarations reachable through `geometry.Name`.
`from geometry import Point as Position` binds just that type under a local name.
You can also write `import geometry as geo`, then use `geo.Point`. These aliases
refer to the same declarations, not copies or new types. Two separate modules
may each define `Point`; those are distinct types.

Imports are absolute. `import tools.geometry` reads `tools/geometry.plenty`
under the source root. Directories provide namespaces and need no `__init__`
file. The root defaults to the entry file's directory. If the entry file is
deeper in the tree, select the root explicitly:

```sh
plenty --module-root src src/app/main.plenty
plenty --module-root src --compile src/app/main.plenty -o app
plenty --module-root src --check-module src/tools/geometry.plenty
```

`--check` checks a complete application and requires `main`. `--check-module`
checks a library and its imports without that requirement. An imported function
named `main` is an ordinary function; importing it does not call it.
