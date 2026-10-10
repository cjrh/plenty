# 27. Split a program into modules

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

Public classes do not automatically expose their fields or methods. Mark each
part of the public API explicitly. Without `__init__`, the generated field
constructor is public only when the class and every field are public. To keep
fields private while allowing construction, declare `pub def __init__`:

Save this example's companion file as `counter.plenty`:

```plenty-file counter.plenty
pub class Counter:
    value: i64

    pub def __init__(self, start: i64) -> ():
        self.value = start

    pub def increment(self: &mut Counter) -> ():
        self.value = self.value + 1

    pub def read(self) -> i64:
        self.value

    def __del__(self) -> ():
        print("counter closed").unwrap()
```

Application:

```plenty
from counter import Counter

def main() -> Result[(), Failure]:
    mut count = Counter(41)
    count.increment()
    print(count.read())?
    Ok(())
```

```output
42
counter closed
```

The private destructor still runs automatically. Private fields and methods are
accessible throughout their defining module, including from helper functions,
but not from another module. Direct reads, writes, and borrows all enforce this.
Here is a rejected example with its own companion file, `secret.plenty`:

```plenty-file secret.plenty
pub class Secret:
    value: i64

    pub def __init__(self, value: i64) -> ():
        self.value = value
```

```plenty-error
from secret import Secret

def main() -> ():
    item = Secret(42)
    print(item.value).unwrap()
```

```error
secret.Secret.value` is private
```

`pub` also applies to functions, enums, and type aliases. A public enum exposes
all its variants. Public signatures cannot mention private classes or enums,
even through aliases or containers. Ordinary imports are private bindings;
importing a name does not re-export it. Relative imports, wildcards, circular
imports, and `pub import` are not supported yet.

Privacy controls direct access, not secrecy: automatic printing and equality
still inspect a class's complete structural value, including private fields.
`pub` does not export a C symbol or change the native calling convention.
