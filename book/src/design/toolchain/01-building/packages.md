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
| `plenty check [--tests] [--locked] [--offline] [--check-all-methods]` | Check the library (without requiring `main`) and every executable, and report [unchecked sources](#check-coverage); [`--tests`](#checking-tests) also checks the [test files](#tests); [`--check-all-methods`](../../language/07-generics/index.md#checking-every-method) also checks unused methods of the package's own generic instances, not those of `std` or dependency types; prints each package's [warnings](warnings.md) once |
| `plenty build [--fast-compile] [--no-strip] [--locked] [--offline]` | Check the library and compile every executable to `target/PROFILE/NAME` |
| `plenty run [--fast-compile] [--no-strip] [--locked] [--offline] [--bin NAME] [-- ARGS...]` | Build one executable and run it with `ARGS` |
| `plenty test [--fast-compile] [--locked] [--offline] [--no-capture] [--seed N] [--list] [--jobs N] [--timeout SECONDS] [FILTER...]` | Compile and run the package's [tests](#tests) |
| `plenty add [ALIAS] SOURCE [--version REQ] [--offline]` | Add a [dependency](#adding-dependencies) to the manifest and lock it |
| `plenty fetch [--locked] [--offline]` | Resolve, download, and lock every dependency without compiling |
| `plenty update [PACKAGE...] [--offline]` | Move [Git dependencies](#git-dependencies) to the commits their tags and revisions name now |
| `plenty graph [--locked] [--offline]` | Print the [package graph file](#package-graph-files) for `plentyc --package-graph` |
| `plenty clean` | Remove `target/` |
| `plenty lsp` | Serve the [language server](language-server.md) on stdin and stdout |

`--locked` fails instead of creating or changing `plenty.lock`. `--offline`
never runs `git fetch`; it uses only the package cache in `.pkg/`. Executables
are [stripped](linking.md#symbols-and-debug-information) of their symbol table
and debug sections in both profiles; `--no-strip` keeps them.

`build --test-hooks` and `run --test-hooks` enable
[failure injection](../../stdlib/test.md#failure-injection) in project
executables. Switching this flag rebuilds the executable. These instrumented
builds retain symbols, like `plenty test`, and are intended for integration tests.

The package name defaults to the directory name and must be a lowercase
identifier other than `std`. Status lines go to standard error.

`plenty run` inherits standard streams, environment, and working directory, and
exits with the program's status; Unix signal termination becomes 128 plus the
signal number. With several executables, `--bin NAME` is required.

Any other first argument is a [compiler command](commands.md): `plenty FILE`
still compiles and runs one file. A file named like a project command needs a
path, such as `plenty ./run`. The `plentyc` binary provides only the compiler
commands.

## Check coverage

A target's check covers the files its import graph reaches: its entry, the
package modules it imports, and the dependency libraries those import.
`plenty check` checks every target, then reports each `.plenty` or `.p` file below the
package's `src/` that no target reached. Such a file was not parsed or
type-checked, so it may contain errors:

```text
  Checking tools v1.0.0
warning: src/util/orphan.plenty is not imported by any target of `tools`; it was not checked
   Checked tools v1.0.0: library and 2 executables; 4 of 5 files in src/
```

The warning does not change the exit status. The summary names the targets
checked and counts the files below `src/` that they reached. Test files are
not targets: `plenty test` checks `tests/` when it compiles them, and
`plenty check` ignores that directory unless given [`--tests`](#checking-tests). Every `.plenty` or `.p` file below `src/` is
package source, which [build reuse](#build-reuse) also fingerprints; keep
fixtures, generated samples and other intentionally unbuilt `.plenty` files in
another directory, such as `tests/fixtures/` or `fixtures/`. Files without the
`.plenty` or `.p` extension are never reported.

### Checking tests

`plenty check --tests` checks everything `plenty check` does, then each test
file directly in `tests/` as `plenty test` would compile it: a file with
`main` as a program, a file of test functions with the test runner's
generated `main`, so a malformed test function's signature is reported too.
It neither links nor runs anything, so a package whose tests need a native
library built first, or whose tests would fail when run, can still be
checked. Errors name the test file, and the summary adds the count, as in
`Checked memmap v0.0.1: library, 1 test file; 1 of 1 files in src/`. The
`src/` coverage count and its warnings still describe the targets only.

`--tests` combines with `--locked`, `--offline` and `--check-all-methods`,
which then also applies to the test files. It takes no test filter. The
single-file `--check FILE` has no `tests/` directory and rejects `--tests`.

## Tests

Test discovery also accepts `.p` files in `tests/`; two spellings with the same
stem are rejected rather than producing duplicate test names.

Each file directly in `tests/` is a test file; files in subdirectories are
helper modules. A test file defines either `main` or `test_` functions, not
both, and defining neither is an error:

- A file with `main` is one test, named by the file stem, that passes when
  the program exits with status zero.
- Each module-level function whose name starts with `test_` is a test named
  `FILE::FUNCTION`. It takes no parameters and returns `()` or
  `Result[(), E]` written out, not through an alias; any other signature is an
  error. It passes when it returns normally or returns `Ok`. A returned `Err`
  is written to standard error as `error: NAME returned ERROR`, except
  `Failure.Unspecified` and `std.test.TestError.Assertion`: the first carries
  nothing to show, and the second follows an explanation `std.test` already
  wrote.

Plenty has no attributes or reflection, so `plenty test` finds test functions
by parsing each file, then compiles the file with a generated `main` that calls
the function named by its one argument. Every test runs in its own process, so
a trap, abort, or leaked state fails or affects only that test. The generated
`main` exits 0 when the test passes and 3 when it returns `Err`; a trap exits 1.

A test file imports the package's library under the package name, the
package's dependencies under their aliases, and its own helpers under `tests/`.
Because those names are dependency aliases, a test file or helper cannot be
named after the package or one of its dependencies.

`plenty test` compiles every selected test file to
`target/PROFILE/tests/PACKAGE/STEM` before running any. These executables also
link the hooks behind [`std.test` failure injection](../../stdlib/test.md#failure-injection),
which wrap the C library's allocation and file calls; `plenty build` and
`plenty run` never link them. Each test runs with the
package directory as its working directory, empty standard input, and `TMPDIR`
set to a new empty directory that the runner removes, with everything in it,
when the test ends. That removal also covers scratch directories of a test
that traps or times out, which skip destructors. The per-test directories live
in one `plenty-test-run-` directory under the runner's own temporary directory.

Tests run in parallel, at most one per available CPU unless `--jobs N` says
otherwise. Results are printed in discovery order whatever order the tests
finish in, so repeated runs print the same lines. Tests that share files in the
package directory must use `--jobs 1` or their own names.

Each test's standard output and error are captured together and printed under
`failures:` only if it fails; `--no-capture` passes them through instead, and
parallel tests' output then interleaves. The command prints one line per test
and a summary, and exits nonzero if any test fails:

```text
running 2 tests
test area::test_square ... ok
test area::test_negative ... FAILED (returned Err)

failures:

---- area::test_negative output ----
assertion failed: left == right
  left: -9
 right: 9

test result: FAILED. 1 passed; 1 failed
```

| Option | Effect |
|---|---|
| `FILTER...` | Run only the tests whose names contain one of the filters; the summary counts the rest as filtered out |
| `--list` | Print the selected test names, one per line, without compiling |
| `--no-capture` | Let test output through as it is written |
| `--seed N` | Set `PLENTY_TEST_SEED=N` for every test; [`std.test`](../../stdlib/test.md) seeds its generator from it |
| `--jobs N` | Run at most `N` tests at once; `N` is at least 1 |
| `--timeout SECONDS` | Kill a test still running after `SECONDS` (at least 1) and fail it as `timed out after SECONDSs`; processes it started are not killed |
| `--fast-compile`, `--locked`, `--offline` | As for `plenty build` |

A failing test's line gives the reason: `returned Err`, `exit status N` (a
trap is status 1), `terminated by signal N`, or `timed out after Ns`. Without
`--timeout` a test can run forever.

## Profiles

| Profile | Selected by | Output | Code generation |
|---|---|---|---|
| optimized | default | `target/optimized/` | Cranelift `opt_level=speed` and optimized call-site passes |
| fast-compile | `--fast-compile` | `target/fast-compile/` | Cranelift `opt_level=none`; skips optimized call-site passes |

Profiles change code generation only. Overflow, bounds, ownership, and every
other language check behave identically; neither profile adds debug information.
Use `--fast-compile` for shorter edit-run cycles in large projects. Measure runtime
performance with the optimized default. Compiled functions, program objects and
executables remain cached separately by profile and by
[CPU level](linking.md#cpu-levels).

`plentyc FILE` and `plentyc --compile FILE -o OUT` also optimize by default;
`--fast-compile` selects the same unoptimized code generation for single files.
`--release` is accepted as a compatibility alias for the optimized default.
These are Plenty profiles, independent of Cargo's profiles for building the compiler.

## Build reuse

`build`, `run`, and `test` skip a target whose output exists and whose inputs
are unchanged since its last successful build; the package's status line then
reads `Fresh` instead of `Compiling`. `build` reuses a library check the same
way; `check` always checks again. The inputs are the name of every file below
each graph package's `src/` (not only the imported ones, so an added module
rebuilds), every file the last build read with its contents, the package graph,
the entry file, the target kind, the profile and linker options, and the
compiler executable (path, size, and modification time; the compiler embeds
`std` and the runtime). A symbolic link below `src/` counts by the name of its
target. The walk does not follow a link to a directory, so a module added
inside a linked directory is noticed only when an import resolves to it, by the
import checks below. The canonical entry and source roots are inputs too, so
moving a package rebuilds. A graph the compiler rejects, such as one with a
[linked source root](#dependencies), is not walked. Reuse makes the checks the [program
cache](../02-architecture/cache.md#program-objects) makes: every import must
still resolve to the same file, and every [included file](#included-files) must
still be one the package may include. A file in the package directory that no
target reads, such as a large data set, costs nothing. Fingerprints live in
`target/PROFILE/.fingerprint/`. A build forgets the old fingerprint before it
starts, so a failed or interrupted build is never reused. Reuse is per target: a
stale target is recompiled whole.

## Manifest

Language editions are not supported; `edition` is an unknown manifest key.
The [editions proposal](../../../proposals/language-editions.md) explores
per-package source compatibility independently of `plenty-version` and
`manifest-version`.

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

[build]
cpu = "x86-64-v3"

[dependencies]
util = { path = "../util", version = "^0.1.0" }

[dependencies.audio]
git = "https://example.com/audio.git"
tag = "v1.2.0"
subdir = "packages/audio"
```

`manifest-version` and `[package]` with `name` and `version` are required.
`version` and `plenty-version` are `MAJOR.MINOR.PATCH`. `plenty-version` is the
minimum toolchain version; an older toolchain rejects the package. The numbers
above are sample package versions and compatibility requirements, not the
current compiler version. `plenty new` and `plenty init` start the new package at
`0.1.0` and set `plenty-version` from the compiler that creates it. Unknown keys
and tables are errors, so a misspelled setting is never ignored. The manifest is
a subset of TOML 1.0: basic and literal strings, decimal integers, booleans,
arrays, tables, inline tables, and arrays of tables. Dotted keys, dates, floats,
and multi-line strings are errors.

`[build] cpu` sets the [CPU level](linking.md#cpu-levels) of the package's
executables and tests: `x86-64` (the default when absent), `x86-64-v2` or
`x86-64-v3`. `--cpu` on `build`, `run`, `test` and `script` overrides it. Only
the package being built chooses: a dependency's `[build]` table is ignored, and
its code is compiled at the level of the program that uses it. A workspace root
without `[package]` rejects `[build]`; set it in each member.

Targets default to `src/lib.plenty` or `src/lib.p` (the library) and
`src/main.plenty` or `src/main.p` (an executable named after the package) when
those files exist. Both spellings of an inferred target are ambiguous; remove
one or declare its path explicitly. `[lib]` and `[[bin]]` replace the defaults;
their paths are `.plenty` or `.p` files below `src/`. A
package needs at least one target. A library entry may define `main`; it is an
ordinary function there.

## Included files

A package may embed any file in its package directory, the directory containing
its `plenty.toml`, with [`include_str` and
`include_bytes`](../../language/02-source/modules.md#included-files). Paths are
relative to the including file, so `src/main.plenty` reads `data/icon.png` as
`include_bytes("../data/icon.png")`. Test files in `tests/` include from the
same directory.

An include reads a file only if its real path, after resolving `..` and symbolic
links, lies inside the package directory; otherwise the error names the package
directory. A link such as `data -> /etc` is judged by its target. The source
root itself cannot be a link (see [Dependencies](#dependencies)).

A file inside a nested directory that holds its own `plenty.toml`, such as a
workspace member's, belongs to that package, and including it is an error that
names both packages. This holds below `src/` too: a `plenty.toml` in a directory
there that no graph package uses blocks includes from that directory, although
imports from it still work. Another package's source root belongs to that
package, even below the includer's package directory; a package and its `tests/`
package share their files. `plenty check` reports unreached `.plenty`/`.p` files only
below `src/`, so another directory can hold `.plenty` text that a program
embeds. The [program cache](../02-architecture/cache.md#program-objects) and
[build reuse](#build-reuse) record each included file's contents and check again
that it may be included.

A [script](scripts.md) owns no package directory. It may include files below its
own directory, even when that directory is inside a package's, and that
package's includes are unaffected.

```text
app/
  plenty.toml
  data/
    icon.png
    sql/schema.sql
  src/
    main.plenty
```

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
of another package except through a declared alias. A package's `src/`, and the
`tests/` of the package being tested, must be real directories, not symbolic
links. This holds for every package, including the one being built:

```text
error: source root /work/util/src is a symbolic link; a package's source root must be a directory
```

Imports and includes are bounded by where the source root really is, so a linked
`src` would let a dependency, including one fetched from Git, read and embed
files from any directory it pointed to. Links below `src/` are allowed; an
import that resolves outside the source root through one is an error. The same module path in two
packages names two modules, so `Point` in two packages is two distinct types.
Diagnostics name a dependency's declarations as `PACKAGE::MODULE.Name`, such
as `util::lib.Point`; the root package's names are unqualified.

## Git dependencies

```toml
[dependencies]
tz = { git = "https://github.com/plentylang/tz.git", tag = "v0.3.0" }
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
commit, and cannot leave the repository. Neither can a `subdir` that is a
symbolic link in the repository: the error says the dependency leaves its
repository snapshot. A graph contains one commit per
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
| A locked `commit` is not 40 lowercase hexadecimal digits | Error: the lockfile is malformed; delete it to resolve again |
| The tag or revision does not exist | Error naming the URL and tag or revision |
| `git` is not installed | Error saying Git dependencies need `git` on `PATH` |

## Adding dependencies

`plenty add` appends one entry to the `[dependencies]` table of the package
containing the current directory, keeping the rest of the file unchanged, then
resolves and locks the graph. If resolution fails, the manifest is restored.

```text
plenty add --path ../util
plenty add tz --git https://github.com/plentylang/tz.git --tag v0.3.0 --version 0.3.0
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
identity whose package has a `library`. A graph names no package directory: each
package's is the directory containing its `root` as written, before symbolic
links are resolved, as for a manifest, whether or not it holds a `plenty.toml`.
As for a manifest, a graph `root` must not be a symbolic link, and its real path
must lie inside that directory.
Its [included files](#included-files) come from there. So a package whose root
is `/work/src` may include any file below `/work`, except inside another
package's directory or source root, or a directory holding a `plenty.toml`.
Relative paths are relative to the file. `--package-graph` works with `FILE`,
`--check`, `--check-module`, and `--compile`. It lets another build system
compile a package without `plenty.toml`, and makes the compiler's input visible
when debugging resolution.
