# Workflow and packages

## Single files

| Command | Use |
|---|---|
| `plenty --check FILE` | Type-check only; fastest feedback; prints warnings, such as `redundant-copy`, on stderr |
| `plenty FILE` | Compile and run |
| `plenty script [OPTIONS] FILE [ARGS...]` | Run with arguments and optional embedded dependencies |
| `plenty --compile FILE -o OUT` | Write a native executable |
| `plenty --fast-compile FILE` | Skip optimization for shorter edit-run cycles |

Profiles change code generation only; every check behaves the same. The separate
`--unchecked trap|assume` option controls plain `unchecked:` arithmetic suites;
keep the default `trap` unless their preconditions have been established. There
is no manifest key for it. See [unchecked arithmetic](errors.md#unchecked-arithmetic). Linking
needs a C compiler driver, `cc`, on `PATH`.

Executable Unix scripts use `#!/usr/bin/env -S plenty script`. All arguments
after FILE belong to the application; script options must precede it. An optional
`---plenty` / `---` TOML header supplies package metadata and path/Git dependencies.
Resolution ignores surrounding workspaces and writes generated state to the Plenty
cache, not beside the source. Use `--lockfile PATH` for a durable lock and
`--locked --offline` for pinned cached resolution. See
[script contracts](../../../../book/src/design/toolchain/01-building/scripts.md).

## Packages

A package is a directory containing `plenty.toml`. `plenty new NAME` creates
an application (`src/main.plenty`); `plenty new NAME --lib` creates a library
(`src/lib.plenty`). `plenty init` does the same in the current directory.

| Command | Use |
|---|---|
| `plenty check` | Type-check the library and every executable; warn about `src/` files no target imports |
| `plenty run [-- ARGS...]` | Build and run the executable |
| `plenty test [FILTER...]` | Run tests in `tests/` |
| `plenty build [--fast-compile]` | Write `target/PROFILE/NAME` |
| `plenty add ALIAS --path DIR` | Add a local dependency |
| `plenty add ALIAS --git URL --tag TAG` | Add a Git dependency |
| `plenty update` | Move Git dependencies to what their tags name now |

The full manifest format, dependency resolution, and test rules are in
[the package reference](../../../../book/src/design/toolchain/01-building/packages.md).

### A library, an application, and a test

The library package `shapes/`:

```toml project-file shapes/plenty.toml
manifest-version = 1

[package]
name = "shapes"
version = "0.1.0"
```

Visibility is per defining module, including within one package: declarations
another module uses must be `pub`, including needed fields and methods. Keep
helpers private unless they form an intentional interface.
Arithmetic errors are part of that public contract too:

```plenty project-file shapes/src/lib.plenty
pub class Square:
    pub side: i64

    pub def area(self) -> Result[i64, OverflowError[i64]]:
        Ok(self.side * self.side)
```

Each file directly in `tests/` is a test file. Functions named `test_*` take
no parameters and return `()` or `Result[(), E]`. A test file imports its own
package's library by the package name:

```plenty project-file shapes/tests/area.plenty
import std.test
from shapes import Square

def test_area() -> Result[(), std.test.TestError | OverflowError[i64]]:
    std.test.equal(&Square(3).area()?, &9)?
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

Files embedded with `include_str`/`include_bytes` go in `assets/` beside
`src/` (or under `src/`); a path is relative to the including file, such as
`include_str("../assets/schema.sql")` from `src/main.plenty`. Includes cannot
read other package files, other packages, or anything a symbolic link points to
outside those two directories. See
[package assets](../../../../book/src/design/toolchain/01-building/packages.md#assets).

Other files under `src/` are the package's own modules, imported by path:

```plenty project-file app/src/report.plenty
pub def show(area: i64) -> Result[(), IoError]:
    print("area:", area)?
    Ok(())
```

The calculation module depends on the geometry library; presentation does not:

```plenty project-file app/src/measure.plenty
import geometry

pub def square_area(side: i64) -> Result[i64, OverflowError[i64]]:
    square = geometry.Square(side)
    square.area()
```

```plenty project-file app/src/main.plenty
from report import show
from measure import square_area

def main() -> Result[(), IoError | OverflowError[i64]]:
    show(square_area(4)?)
```

```sh plenty-project app
plenty check
```
```output
```

```sh plenty-project app
plenty run
```

```output
area: 16
```

Status lines such as `Compiling` go to standard error. The first build writes
`plenty.lock`; commit it with the manifest.

### Splitting a package by responsibility

The example has one-way imports: `app/main -> measure -> geometry`, and
`app/main -> report`. Split parsing, application policy and presentation when
they have distinct responsibilities or contracts, rather than imposing a line
limit. Keep shared types in a lower-level module both consumers can import.
Imports do not re-export names: import a type from its defining module instead
of relying on another file's import. Cycles are rejected, with the dependency
chain in the diagnostic; moving a shared type or passing a value can break one.
See [modules and imports](../../../../book/src/design/language/02-source/modules.md).

Start a move by deciding the smallest public interface. A moved helper does
not automatically need to be public; expose only what callers in other modules
use. Public classes also need explicit visibility on externally used fields
and methods. Preserve error types and ownership in the new signatures.

After each move, check the package library and every executable with
`plenty check` and resolve any warning about a `src/` file that no target
imports, run `plenty test` for its tests, and run the relevant binaries.
In this walkthrough, run `plenty test` in `shapes/` and `plenty check` / `plenty run`
in `app/`. A standalone `plenty --check src/main.plenty` checks that entry's
reachable graph; it does not establish coverage of a separate library or other
binaries. Files not reachable from any target/test can still be orphaned: an
unimported file is not checked merely because it is under `src/`. Connect the
new module to an appropriate target or test before trusting a successful check.
<!-- issue #805 -->

The [language server](../../../../book/src/design/toolchain/01-building/language-server.md)
provides diagnostics, hover, go to definition, inlay hints and completion; see
[Neovim setup](../../../../book/src/design/toolchain/01-building/neovim.md).
References/rename and move-to-module refactoring are not shipped capabilities.
Use explicit edits and package checks rather than assuming editor support.
<!-- issues #821 and #822 -->

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
  contracts are in [builtins](../../../../book/src/design/facilities/01-builtins/index.md) and [I/O](../../../../book/src/design/facilities/02-io/index.md).
- Standard modules (`import std.json`, `from std.math import max`): search
  [the signature index](../../../../book/src/design/stdlib/signatures.md) for a name or return type, then read
  that module's page under `std/` for errors, ownership, and limits.
- Signatures show ownership: `&T` borrows, `&mut T` borrows exclusively, a
  bare `T` takes ownership. A `Result[..., E]` names exactly the errors a
  caller must handle or propagate.

For filesystem links, use `std.fs.symlink(&target, &link)` and
`std.fs.canonicalize(&path)` with native-byte `std.path.Path` values. Both work
on workers; canonicalization requires existing components and does not secure a
later open against concurrent replacement. Read the
[filesystem reference](../../../../book/src/design/stdlib/fs/metadata.md#creating-and-resolving-links) for path limits and native errors before replacing a rooted handle operation.

For private filesystem state, `std.fs.Root.create_file` exclusively creates a
read/write file with default mode 0600; `Root.create_dir` defaults to 0700.
Both accept `unix_mode`, restricted by the OS creation policy. Change permissions
through the held handle using `Root.set_unix_mode` or `std.fs.set_file_unix_mode`.
Read the [rooted filesystem contracts](../../../../book/src/design/stdlib/fs/helpers.md)
before relying on symlink, umask, existing-path or platform behavior. Use
`temporary_file` and publication when incomplete writes must stay unnamed.
