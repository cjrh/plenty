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

def main() -> Result[(), AllocError | IoError]:
    square = geometry.Square(4)
    show(square.area())
```

Other files under `src/` are the package's own modules, imported as before:

```plenty project-file app/src/report.plenty
pub def show(area: i64) -> Result[(), IoError]:
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
A later build with no changed source reports `Fresh` and reuses the executable.

The first build also writes `plenty.lock`, which records the resolved packages.
Commit it with the manifest. `--locked` makes a build fail instead of changing
it.

A dependency can also come from a Git repository, at a tag or a commit. The
official [time-zone package](../../design/stdlib/time.md#civil-time-and-time-zones)
is one:

```toml
[dependencies]
tz = { git = "https://github.com/plentylang/tz.git", tag = "v0.2.0", version = "0.2.0" }
```

The first build fetches the repository into `.pkg/` and records the exact
commit in `plenty.lock`. Later builds use that commit even if the tag moves
upstream, until `plenty update` resolves it again. The optional `version`
checks that the dependency's own version is compatible. `plenty add` writes
these entries and updates the lockfile:

```text
plenty add geometry --path ../shapes
plenty add tz --git https://github.com/plentylang/tz.git --tag v0.2.0
```

Several related packages can share one lockfile and `target/` as a
[workspace](../../design/toolchain/01-building/packages.md#workspaces).

A dependency's name belongs only to the package that declares it. `app` cannot
import what `shapes` depends on without declaring it too, and a local module
named `geometry` would be an error. Two packages may each define `Square`;
those are distinct types.

The dependency's name also reaches its other modules. If `shapes` had
`src/circles.plenty`, `import geometry.circles` would bind it beside
`import geometry`, unless the library itself declares something named
`circles`; [modules and imports](../../design/language/02-source/modules.md)
gives the rule.

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
