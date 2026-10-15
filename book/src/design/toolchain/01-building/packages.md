# Packages and project commands

A package is a directory containing `plenty.toml`. Project commands search the
current directory and its parents for the nearest manifest, then for an
enclosing [workspace](#workspaces). The compiler receives a fixed package
graph; imports never fetch or resolve packages.

## Commands

| Command | Behavior |
|---|---|
| `plenty new PATH [--lib] [--name NAME]` | Create `PATH` with a manifest, `src/main.plenty` (or `src/lib.plenty`), and `.gitignore` |
| `plenty init [--lib] [--name NAME]` | Create a package in the current directory; refuses to replace a manifest or source |
| `plenty check [--locked] [--offline]` | Check the library (without requiring `main`) and every executable |
| `plenty build [--release] [--locked] [--offline]` | Check the library and compile every executable to `target/PROFILE/NAME` |
| `plenty run [--release] [--locked] [--offline] [--bin NAME] [-- ARGS...]` | Build one executable and run it with `ARGS` |
| `plenty test [--release] [--locked] [--offline] [--no-capture] [--seed N] [--list] [FILTER...]` | Compile and run the package's [tests](#tests) |
| `plenty add [ALIAS] SOURCE [--version REQ] [--offline]` | Add a [dependency](#adding-dependencies) to the manifest and lock it |
| `plenty fetch [--locked] [--offline]` | Resolve, download, and lock every dependency without compiling |
| `plenty update [PACKAGE...] [--offline]` | Move [Git dependencies](#git-dependencies) to the commits their tags and revisions name now |
| `plenty graph [--locked] [--offline]` | Print the [package graph file](#package-graph-files) for `plentyc --package-graph` |
| `plenty clean` | Remove `target/` |

`--locked` fails instead of creating or changing `plenty.lock`. `--offline`
never runs `git fetch`; it uses only the package cache in `.pkg/`.

The package name defaults to the directory name and must be a lowercase
identifier other than `std`. Status lines go to standard error.

`plenty run` inherits standard streams, environment, and working directory, and
exits with the program's status; Unix signal termination becomes 128 plus the
signal number. With several executables, `--bin NAME` is required.

Any other first argument is a [compiler command](commands.md): `plenty FILE`
still compiles and runs one file. A file named like a project command needs a
path, such as `plenty ./run`. The `plentyc` binary provides only the compiler
commands.

## Tests

Each file directly in `tests/` is a test file; files in subdirectories are
helper modules. A test file defines either `main` or `test_` functions, not
both, and defining neither is an error:

- A file with `main` is one test, named by the file stem, that passes when
  the program exits with status zero.
- Each module-level function whose name starts with `test_` is a test named
  `FILE::FUNCTION`. It takes no parameters and returns `()` or
  `Result[(), E]` written out, not through an alias; any other signature is an
  error. It passes when it returns normally or returns `Ok`. A returned `Err`
  is written to standard error as `error: NAME returned ERROR`.

Plenty has no attributes or reflection, so `plenty test` finds test functions
by parsing each file, then compiles the file with a generated `main` that calls
the function named by its one argument. Every test runs in its own process, one
at a time, so a trap, abort, or leaked state fails or affects only that test.

A test file imports the package's library under the package name, the
package's dependencies under their aliases, and its own helpers under `tests/`.
Because those names are dependency aliases, a test file or helper cannot be
named after the package or one of its dependencies.

`plenty test` compiles every selected test file to
`target/PROFILE/tests/PACKAGE/STEM` before running any. Each test runs with the package directory as its working
directory and empty standard input. Its standard output and error are
captured together and printed under `failures:` only if it fails;
`--no-capture` passes them through instead. The command prints one line per
test and a summary, and exits nonzero if any test fails:

```text
running 2 tests
test area::test_square ... ok
test area::test_negative ... FAILED (exit status 1)

failures:

---- area::test_negative output ----
assertion failed: left == right
  left: -9
 right: 9
error: test_negative returned Failure.Unspecified

test result: FAILED. 1 passed; 1 failed
```

| Option | Effect |
|---|---|
| `FILTER...` | Run only the tests whose names contain one of the filters; the summary counts the rest as filtered out |
| `--list` | Print the selected test names, one per line, without compiling |
| `--no-capture` | Let test output through as it is written |
| `--seed N` | Set `PLENTY_TEST_SEED=N` for every test; [`std.test`](../../stdlib/test.md) seeds its generator from it |
| `--release`, `--locked`, `--offline` | As for `plenty build` |

Tests run sequentially and have no timeout.

## Profiles

| Profile | Selected by | Output | Code generation |
|---|---|---|---|
| debug | default | `target/debug/` | Cranelift `opt_level=none`; fastest compilation |
| release | `--release` | `target/release/` | Cranelift `opt_level=speed` |

Profiles change code generation only. Overflow, bounds, ownership, and every
other language check behave identically. `plentyc --release FILE` and
`plentyc --release --compile FILE -o OUT` select the release code generation for
single files.

## Build reuse

`build`, `run`, and `test` skip a target whose output exists and whose inputs
are unchanged since its last successful build; the package's status line then
reads `Fresh` instead of `Compiling`. `check` and `build` reuse a library check
the same way. The inputs are the content of every file below each graph
package's `src/` (not only the imported ones), the package graph, the entry
file, the target kind, the profile and linker options, and the compiler
executable (path, size, and modification time; the compiler embeds `std` and
the runtime). Fingerprints live in `target/PROFILE/.fingerprint/`. A build
forgets the old fingerprint before it starts, so a failed or interrupted build
is never reused. Reuse is per target: a stale target is recompiled whole.

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
util = { path = "../util", version = "^0.1.0" }

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
containing `plenty.toml`. A `git` dependency is described [below](#git-dependencies).

`version` is an optional requirement on the dependency's `version`, checked
after resolution; it never selects a version. `X.Y.Z` and `^X.Y.Z` accept
versions at least `X.Y.Z` with the same leftmost nonzero component (`^0.2.1`
accepts `0.2.5` but not `0.3.0`), `=X.Y.Z` accepts only that version, and
`>=X.Y.Z` accepts that version or later. A mismatch names both packages:

```text
error: package `app` requires `util` ^2.0.0, but /work/util is version 1.4.0
```

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

## Git dependencies

```toml
[dependencies]
audio = { git = "https://example.com/audio.git", tag = "v1.2.0" }
codec = { git = "https://example.com/media.git", rev = "4f2a9c1", subdir = "codec" }
```

A `git` dependency needs exactly one of `tag` (a tag name) or `rev` (7 to 40
lowercase hexadecimal digits of a commit). `subdir` is the package's directory
inside the repository, relative and without `..`. Plenty runs the system `git`
program; any URL it accepts works, including `file://` and SSH. Credentials
come from Git's own configuration; Plenty never prompts.

The first resolution fetches every branch and tag of the repository into a
bare repository under `.pkg/git/db/`, records the commit that the tag or
revision names in `plenty.lock`, and extracts that commit's tree into
`.pkg/git/checkouts/REPO/COMMIT/`. Later commands build the locked commit,
even if the tag has moved upstream; they fetch only when the cache lacks the
commit, as after deleting `.pkg/`. Each repository is fetched at most once per
command. Snapshots are extracted beside their final path and renamed into
place, so an interrupted fetch never leaves a partial snapshot. `.pkg/` is
generated: the `.gitignore` that `plenty new` writes excludes it.

`plenty update` resolves every tag and revision again and rewrites the
lockfile; `plenty update NAME...` resolves only the named packages. It prints
each changed commit.

A `path` dependency inside a Git package names another package of the same
commit, and cannot leave the repository. A graph contains one commit per
package: if two dependents reach the same repository directory at different
commits, resolution fails and names both chains:

```text
error: conflicting requirements for package `geo` from https://example.com/mono.git:
  app requires tag=v2 (commit c25fe24762e9)
  app -> shapes requires commit 421b7a9fd85a through a path in the same repository
a graph may contain one commit per package
```

| Situation | Behavior |
|---|---|
| `--offline` and the repository or commit is not cached | Error naming the URL; nothing is fetched |
| `--locked` and the lockfile has no commit for a requirement | Error before any fetch |
| The locked commit is gone upstream and not cached | Error suggesting `plenty update` |
| The tag or revision does not exist | Error naming the URL and tag or revision |
| `git` is not installed | Error saying Git dependencies need `git` on `PATH` |

## Adding dependencies

`plenty add` appends one entry to the `[dependencies]` table of the package
containing the current directory, keeping the rest of the file unchanged, then
resolves and locks the graph. If resolution fails, the manifest is restored.

```text
plenty add --path ../util
plenty add audio --git https://example.com/audio.git --tag v1.2.0 --version 1.2.0
plenty add codec --git https://example.com/media.git --rev 4f2a9c1 --subdir codec
```

`--path` is relative to the current directory and is written relative to the
manifest. The alias defaults to the dependency's package name; adding an alias
that already exists is an error.

## Workspaces

A workspace is a directory whose `plenty.toml` lists member packages:

```toml
manifest-version = 1

[workspace]
members = ["app", "util"]
```

Members are directories below the workspace, without `..`. The same manifest
may also contain `[package]`, which makes the workspace directory a member too.
Members share one `plenty.lock`, one `.pkg/`, and one `target/` in the
workspace directory, and depend on each other through ordinary `path`
dependencies.

A command run in the workspace directory acts on every member: `check` and
`build` process each in manifest order, `run` chooses among all their
executables (whose names must be unique), and `test` prefixes each test name
with `PACKAGE/`. A command run inside a member acts on that member only, with
the shared lockfile and outputs. `plenty add` and `plenty graph` need a member.

## Lockfile

Every project command writes `plenty.lock` beside the root manifest (or in the
workspace directory) when it is missing or out of date. It lists each
package's name, version, source, and dependencies as alias/package pairs: the
root package first, then the others sorted by name. A source is `root`,
`path+RELATIVE` from the lockfile's directory, or `git+URL` with the `subdir`,
the full `commit`, and the `selectors` (such as `tag=v1.2.0`) that resolved to
it. It contains no absolute paths, so a checkout can move. Writes replace the
file atomically. `--locked` fails instead of creating or changing it. A
lockfile that cannot be read is an error; deleting it resolves every
dependency again.

## Package graph files

`plenty graph` prints the resolved graph of the current package as TOML, which
`plentyc --package-graph FILE` reads in place of `--module-root`:

```toml
graph-version = 1

[[package]]
identity = "app"
root = "/work/app/src"
dependencies = { util = "util" }

[[package]]
identity = "util"
root = "/work/util/src"
library = "/work/util/src/lib.plenty"
dependencies = {}
```

The first package contains the entry file. `identity` is unique and qualifies
the package's declarations in diagnostics; `dependencies` maps each alias to an
identity whose package has a `library`. Relative paths are relative to the
file. `--package-graph` works with `FILE`, `--check`, `--check-module`, and
`--compile`. It lets another build system compile a package without
`plenty.toml`, and makes the compiler's input visible when debugging
resolution.
