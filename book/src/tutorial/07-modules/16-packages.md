# Build a package with dependencies

A package is a directory with a `plenty.toml` manifest and sources under
`src/`. `plenty new hello` creates one; `plenty new shapes --lib` creates a
library. Here is a library package in `shapes/`:

```toml project-file shapes/plenty.toml
manifest-version = 1

[package]
name = "shapes"
version = "0.1.0"
```

`src/lib.plenty` is the library: the module that other packages import.

```plenty project-file shapes/src/lib.plenty
pub class Square:
    pub side: i64

    pub def area(self) -> i64:
        self.side * self.side
```

An application beside it declares the library as a dependency. The key under
`[dependencies]` is the name that application code imports, and `path` is
relative to this manifest:

```toml project-file app/plenty.toml
manifest-version = 1

[package]
name = "app"
version = "0.1.0"

[dependencies]
geometry = { path = "../shapes" }
```

`src/main.plenty` is the executable, named after the package:

```plenty project-file app/src/main.plenty
import geometry
from report import show

def main() -> Result[(), Failure]:
    square = geometry.Square(4)
    show(square.area())
```

Other files under `src/` are the package's own modules, imported as before:

```plenty project-file app/src/report.plenty
pub def show(area: i64) -> Result[(), Failure]:
    print("area:")?
    print(area)?
    Ok(())
```

Run `plenty run` anywhere inside `app/`:

```sh plenty-project app
plenty run
```

```output
area:
16
```

`plenty run` compiles into `target/debug/app` and runs it. Progress messages go
to standard error; program arguments follow `--`, as in `plenty run -- a b`.
`plenty build --release` writes an optimized executable to `target/release/`.

The first build also writes `plenty.lock`, which records the resolved packages.
Commit it with the manifest. `--locked` makes a build fail instead of changing
it.

A dependency's name belongs only to the package that declares it. `app` cannot
import what `shapes` depends on without declaring it too, and a local module
named `geometry` would be an error. Two packages may each define `Square`;
those are distinct types.

Programs in `tests/` are integration tests. Each one sees the package's library
under the package name, and passes when `main` returns exit status zero:

```plenty project-file shapes/tests/area.plenty
from shapes import Square

def main() -> i32:
    if Square(3).area() == 9:
        0
    else:
        1
```

```sh plenty-project shapes
plenty test
```

```output
running 1 test
test area ... ok

test result: ok. 1 passed; 0 failed
```

[A later lesson](18-standard-test.md) shows test functions and `std.test`.
`plenty check` type-checks every target without linking. `plenty clean`
removes `target/`. Single files still run with `plenty FILE`; the compiler is
also available alone as `plentyc`.
