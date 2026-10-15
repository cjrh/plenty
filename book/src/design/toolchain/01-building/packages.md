# Packages and project commands

A package is a directory containing `plenty.toml`. Project commands search the
current directory and its parents for the nearest manifest. The compiler
receives a fixed package graph; imports never fetch or resolve packages.

## Commands

| Command | Behavior |
|---|---|
| `plenty new PATH [--lib] [--name NAME]` | Create `PATH` with a manifest, `src/main.plenty` (or `src/lib.plenty`), and `.gitignore` |
| `plenty init [--lib] [--name NAME]` | Create a package in the current directory; refuses to replace a manifest or source |
| `plenty check [--locked]` | Check the library (without requiring `main`) and every executable |
| `plenty build [--release] [--locked]` | Check the library and compile every executable to `target/PROFILE/NAME` |
| `plenty run [--release] [--locked] [--bin NAME] [-- ARGS...]` | Build one executable and run it with `ARGS` |
| `plenty test [--release] [--locked]` | Compile and run each `tests/*.plenty` program |
| `plenty clean` | Remove `target/` |

The package name defaults to the directory name and must be a lowercase
identifier other than `std`. Status lines go to standard error.

`plenty run` inherits standard streams, environment, and working directory, and
exits with the program's status; Unix signal termination becomes 128 plus the
signal number. With several executables, `--bin NAME` is required.

`plenty test` compiles every test before running any. Each test runs with the
package directory as its working directory and passes when it exits with status
zero. Files in subdirectories of `tests/` are helper modules, not tests. A test
imports the package's library under the package name, the package's
dependencies under their aliases, and its own helpers under `tests/`. Because
those names are dependency aliases, a test file or helper cannot be named after the
package or one of its dependencies. The
command exits nonzero if any test fails.

Any other first argument is a [compiler command](commands.md): `plenty FILE`
still compiles and runs one file. A file named like a project command needs a
path, such as `plenty ./run`. The `plentyc` binary provides only the compiler
commands.

## Profiles

| Profile | Selected by | Output | Code generation |
|---|---|---|---|
| debug | default | `target/debug/` | Cranelift `opt_level=none`; fastest compilation |
| release | `--release` | `target/release/` | Cranelift `opt_level=speed` |

Profiles change code generation only. Overflow, bounds, ownership, and every
other language check behave identically. `plentyc --release FILE` and
`plentyc --release --compile FILE -o OUT` select the release code generation for
single files. Every command rebuilds its targets; there is no incremental cache.

## Manifest

```toml
manifest-version = 1

[package]
name = "app"
version = "0.1.0"
description = "optional"
authors = ["optional"]
license = "optional"
plenty-version = "0.1.0"

[lib]
path = "src/lib.plenty"

[[bin]]
name = "app"
path = "src/main.plenty"

[dependencies]
util = { path = "../util" }

[dependencies.audio]
git = "https://example.com/audio.git"
tag = "v1.2.0"
subdir = "packages/audio"
```

`manifest-version` and `[package]` with `name` and `version` are required.
`version` and `plenty-version` are `MAJOR.MINOR.PATCH`. `plenty-version` is the
minimum toolchain version; an older toolchain rejects the package. Unknown keys
and tables are errors, so a misspelled setting is never ignored. The manifest is
a subset of TOML 1.0: basic and literal strings, decimal integers, booleans,
arrays, tables, inline tables, and arrays of tables. Dotted keys, dates, floats,
and multi-line strings are errors.

Targets default to `src/lib.plenty` (the library) and `src/main.plenty` (an
executable named after the package) when those files exist. `[lib]` and
`[[bin]]` replace the defaults; their paths are `.plenty` files below `src/`. A
package needs at least one target. A library entry may define `main`; it is an
ordinary function there.

## Dependencies

Each `[dependencies]` key is an alias: the only name under which that package's
code imports the dependency's library. `import util` binds the library module,
and `import util.shapes` reads `src/shapes.plenty` in the dependency. Aliases are
lowercase identifiers. `std` cannot be an alias, and an alias that matches a
local top-level module or directory is an error.

`path` is relative to the declaring manifest and names a live directory
containing `plenty.toml`. `git` dependencies require exactly one of `tag` or
`rev` and accept `subdir`; they are validated but not yet fetched, so a build
that reaches one fails.

Resolution reads every manifest before compiling. A graph contains one package
per name: two dependency paths reaching the same directory share one package
and its types, whatever their aliases; two different directories with the same
package name are an error that names both paths. Package cycles are errors.
Dependencies are not transitive: a package imports only what it declares.

Each package's sources live in its own `src/` directory and cannot import files
of another package except through a declared alias. The same module path in two
packages names two modules, so `Point` in two packages is two distinct types.
Diagnostics name a dependency's declarations as `PACKAGE::MODULE.Name`, such
as `util::lib.Point`; the root package's names are unqualified.

## Lockfile

Every project command writes `plenty.lock` beside the root manifest when it is
missing or out of date. It lists each package's name, version, source (`root`
or `path+RELATIVE` from the root package), and dependencies as alias/package
pairs, sorted by name. It contains no absolute paths, so a checkout can move.
Writes replace the file atomically. `--locked` fails instead of creating or
changing it.
