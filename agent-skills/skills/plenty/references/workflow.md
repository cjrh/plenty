# Workflow and packages

## Single files

| Command | Use |
|---|---|
| `plenty --check FILE` | Type-check only; fastest feedback |
| `plenty FILE` | Compile and run |
| `plenty --compile FILE -o OUT` | Write a native executable |
| `plenty --release FILE` | Run with optimized code generation |

Profiles change code generation only; every check behaves the same. Linking
needs a C compiler driver, `cc`, on `PATH`.

## Packages

A package is a directory containing `plenty.toml`. `plenty new NAME` creates
an application (`src/main.plenty`); `plenty new NAME --lib` creates a library
(`src/lib.plenty`). `plenty init` does the same in the current directory.

| Command | Use |
|---|---|
| `plenty check` | Type-check the library and every executable |
| `plenty run [-- ARGS...]` | Build and run the executable |
| `plenty test [FILTER...]` | Run tests in `tests/` |
| `plenty build [--release]` | Write `target/PROFILE/NAME` |
| `plenty add ALIAS --path DIR` | Add a local dependency |
| `plenty add ALIAS --git URL --tag TAG` | Add a Git dependency |
| `plenty update` | Move Git dependencies to what their tags name now |

The full manifest format, dependency resolution, and test rules are in
[the package reference](packages.md).

### A library, an application, and a test

The library package `shapes/`:

```toml project-file shapes/plenty.toml
manifest-version = 1

[package]
name = "shapes"
version = "0.1.0"
```

Items other packages use must be `pub`, including fields and methods:

```plenty project-file shapes/src/lib.plenty
pub class Square:
    pub side: i64

    pub def area(self) -> i64:
        self.side * self.side
```

Each file directly in `tests/` is a test file. Functions named `test_*` take
no parameters and return `()` or `Result[(), E]`. A test file imports its own
package's library by the package name:

```plenty project-file shapes/tests/area.plenty
import std.test
from shapes import Square

def test_area() -> Result[(), std.test.TestError]:
    std.test.equal(&Square(3).area(), &9)?
    Ok(())
```

```sh plenty-project shapes
plenty test
```

```output
running 1 test
test area::test_area ... ok

test result: ok. 1 passed; 0 failed
```

The application `app/` depends on it. The `[dependencies]` key is the alias
the application imports; `path` is relative to this manifest:

```toml project-file app/plenty.toml
manifest-version = 1

[package]
name = "app"
version = "0.1.0"

[dependencies]
geometry = { path = "../shapes" }
```

Other files under `src/` are the package's own modules, imported by path:

```plenty project-file app/src/report.plenty
pub def show(area: i64) -> Result[(), IoError]:
    print("area:", area)?
    Ok(())
```

```plenty project-file app/src/main.plenty
import geometry
from report import show

def main() -> Result[(), IoError]:
    square = geometry.Square(4)
    show(square.area())
```

```sh plenty-project app
plenty run
```

```output
area: 16
```

Status lines such as `Compiling` go to standard error. The first build writes
`plenty.lock`; commit it with the manifest.

### Git dependencies

```toml
[dependencies]
tz = { git = "https://github.com/plentylang/tz.git", tag = "v0.2.0", version = "0.2.0" }
```

A Git dependency names exactly one of `tag` or `rev` (a commit), and
optionally `subdir`. The first build fetches into `.pkg/` and locks the commit
in `plenty.lock`; later builds use the locked commit until `plenty update`.
`--offline` never fetches; `--locked` fails rather than change the lockfile.
Dependencies are not transitive: import only what your manifest declares.

## Finding APIs

- Prelude names (no import): `print`, `len`, `range`, `copy`, `drop`, `iter`,
  `args`, `input`, `read_text`, `write_text`, `open`, `write_stderr`, and the
  types `list`, `dict`, `set`, `str`, `Option`, `Result`, `Box`. Their
  contracts are in [builtins](builtins/index.md) and [I/O](io/index.md).
- Standard modules (`import std.json`, `from std.math import max`): search
  [the signature index](std/signatures.md) for a name or return type, then read
  that module's page under `std/` for errors, ownership, and limits.
- Signatures show ownership: `&T` borrows, `&mut T` borrows exclusively, a
  bare `T` takes ownership. A `Result[..., E]` names exactly the errors a
  caller must handle or propagate.
