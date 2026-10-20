# Single-file scripts with embedded manifests

> **Status: Not implemented.** Proposed under [B17](../backlog.md):
> `plenty script`, optional embedded manifests, and reusable script builds.
> Ordinary [single-file compilation](../design/toolchain/01-building/commands.md)
> and [package resolution](../design/toolchain/01-building/packages.md) already
> exist. This proposal preserves native compilation and explicit `main`.
> Commands and headers below are candidate syntax, not executable lessons.

## Motivation and precedent

The separate [language editions proposal](language-editions.md) considers an
optional package edition that could also appear in this header. Neither
proposal depends on implementing the other.

A small utility should be one file that can be copied, inspected, marked
executable, and run. Requiring a directory, manifest and wrapper just to use a
dependency discourages that workflow. The same file should remain ordinary
Plenty, with static checking and the normal ownership and error rules.

The motivating example is the local `~/recipes/wtnew.rs`: a worktree utility
with a `cargo +nightly -Zscript` shebang and a `---cargo` TOML dependency header.
Its Git operations are application logic, not build steps; Plenty does not
need a special command-execution language for this workflow.

[Rust's experimental RFC 3424](https://rust-lang.github.io/rfcs/3424-cargo-script.html)
motivates optional manifests, reuse of package workflows, independence from
surrounding workspaces, and diagnostics in the original file. Its examples use
doc-comment fences. The [Cargo Book's experimental script documentation](https://doc.rust-lang.org/cargo/reference/unstable.html#script)
instead shows `---cargo` frontmatter, as does the local example. These are
design precedents, not a settled syntax specification for Plenty.

## The user-facing shape

The explicit command is `plenty script`. A Unix executable script starts with:

```text
#!/usr/bin/env -S plenty script

def main() -> Result[(), IoError]:
    print("hello from a script")?
    Ok(())
```

```text
chmod +x hello.plenty
./hello.plenty
plenty script hello.plenty
```

No manifest is needed for the embedded standard library. The shebang is optional
when invoking the command explicitly. `env -S` passes the subcommand; where it
is unavailable, invoke `plenty script` directly. The compiler and supported
native linker environment must still be installed.

For package metadata and dependencies, add a header after the optional shebang:

```text
#!/usr/bin/env -S plenty script
---plenty
manifest-version = 1

[package]
name = "worktree_tool"
version = "0.1.0"
description = "A small repository helper"

[dependencies]
util = { path = "../util", version = "^0.1.0" }
---

import util

def main() -> Result[(), IoError]:
    print("helper loaded")?
    Ok(())
```

Here `../util` is an existing Plenty library package with its own `plenty.toml`,
not a bundled module. Git dependencies use the existing `git` plus `tag` or
`rev` forms. This introduces neither a registry nor version-only lookup.

### Invocation and arguments

```text
plenty script [OPTIONS] [--] FILE [ARGS...]
plenty script --offline tool.plenty --help
plenty script --check tool.plenty
plenty script --compile -o ./tool tool.plenty
```

Runner options precede `FILE`. Everything after it goes unchanged to the
program, including `--help`, `--release`, and a literal `--`. The optional `--`
before `FILE` permits filenames beginning with `-`. Thus `./tool.plenty --help`
asks the program for help; `plenty script --help` asks the runner. Preserve
native argument bytes, matching [`args_bytes`](../design/facilities/02-io/streams-args.md).

Default compile-and-run, `--check`, and `--compile -o OUT` share resolution and
diagnostics. Check and compile modes reject program arguments and never run
the application. `--release`, `--offline`, `--locked`, and `--verbose` retain
their existing meanings where applicable. The default profile is debug.
Build status goes to stderr; a fresh cached run is quiet.

Inherit the caller's working directory, environment and streams. Expose the
invoked script path as argument zero rather than the cached executable;
retained binaries use normal executable argument zero. Preserve existing exit
and signal-status behavior. Do not change directories to resolve imports or
interpret the shebang as shell code.

## Manifest and source rules

Reuse the strict TOML subset and dependency schema. A script adapter supplies
defaults without weakening ordinary `plenty.toml` validation. An absent header
means an empty manifest; absent `manifest-version` means 1. `[package]` is
optional: default version `0.0.0`, and derive a lowercase name from the file
stem, replacing invalid characters with `_` and prefixing `script_` when needed
for a valid nonreserved identifier. Explicit names follow package validation.
Support description, authors, license and minimum `plenty-version`; do not
automatically install or switch compilers.

There is exactly one executable target: this file. Reject `[workspace]`,
`[lib]`, `[[bin]]`, unsupported tables and unknown keys. Do not infer neighboring
targets or join an enclosing workspace. Keep `plenty FILE` and package commands
unchanged; `plentyc` remains a compiler, without dependency fetching.

Recognize one `---plenty` opener and one `---` closer before any declarations,
allowing blank lines before the header. Unterminated, duplicate or misplaced
headers are errors. Preserve original filenames, byte spans and line numbers
for both TOML and Plenty diagnostics, including CRLF files. Never silently
ignore a malformed header. Imported modules cannot carry script manifests;
package metadata belongs to the selected root.

Resolve the script's physical path once. Its parent is the module root and
base for relative dependencies; a symlink launcher refers to resources beside
its target. Permit sibling-module imports, but do not discover dependencies
from the caller's directory or a nearby `plenty.toml`. Normal alias-conflict
and cycle rules apply. Sibling modules and path dependencies make a script
convenient locally but no longer self-contained when copied elsewhere.

## Storage, locking, and reproducibility

Keep generated files out of the script directory, including when it is
read-only. Use private per-user storage with a namespace derived from the
resolved script path; equal basenames must not share locks or outputs. This
needs explicit storage roots in the package layer, rather than a generated
project that loses the original source paths.

Recommend a cached dependency lock, reused across source edits. Dependency
changes reconcile it using normal resolution rules. `--locked` refuses to
create or change it, before fetching. `--offline` uses only cached sources.
With no dependencies, an empty graph needs no lockfile and works with both
flags on its first run.

A cached lock is not a portable reproducibility guarantee: deleting it or
copying/moving the script can resolve a moved tag differently. Recommend an
explicit `--lockfile PATH` for a durable, shareable lock. The option path is
relative to the caller, while paths recorded inside the lock are relative to
the script directory. The same locked/offline rules apply. Full Git revisions
alone do not pin mutable tags in transitive dependencies; path dependencies
remain live trees. Durable locks must not be removed by cache cleanup.

Executable reuse validates script/import contents, dependency graph,
compiler/runtime identity, target, profile and linker inputs. Object/function
caches help but do not themselves remove repeated linking. Concurrent launches
need per-entry locking and atomic publication: never execute partial or failed
builds, or replace a running executable in place. Include script outputs in
cache inspection and cleanup.

## Boundaries and alternatives

This is native compile-and-run convenience, not an interpreter, REPL, implicit
top-level execution, new error policy or sandbox. Dependencies carry the same
trust implications as ordinary package builds. No expression mode, stdin
source, executable header hooks, library publication, or automatic header
rewriting is included. Running a script never edits its source.

A `#`-commented TOML block avoids new source framing, but older compilers could
silently ignore it. Raw frontmatter makes metadata explicit and older compilers
reject it; the cost is a shared header reader and editor support. Implicitly
enhancing `plenty FILE` saves a word but mixes the existing compiler shortcut
with dependency resolution and persistent artifacts. Always placing locks
beside scripts improves discoverability but breaks read-only directories.

To grow into a package, move the body to `src/main.plenty` and the header to
`plenty.toml`, filling required fields and adjusting relative dependency paths.
No language rewrite is needed. Existing module and entry-point rationale is
recorded in [Explicit applications and modules](entrypoints-modules-and-tutorials.md).

## Implementation assessment and acceptance

This is a moderate toolchain change, not a borrow-checker or code-generation
project. A shebang-only prototype is small: at `daf964d1`, a real
`#!/usr/bin/env plenty` program compiled and printed through `plenty FILE`;
adding `--help` after its filename was rejected. The full proposal needs
manifest adaptation, argument forwarding, isolated resolution/storage, source
mapping and executable reuse, rather than just a CLI wrapper.

Likely boundaries are `src/main.rs`, `src/compiler_cli.rs`, `src/project.rs`,
`src/project/{manifest,toml,git,fingerprint}.rs`, and the frontend source
loader/LSP. Reuse package resolution behind explicit source/storage inputs.
An unresolved header in `plentyc` should direct users to `plenty script`, not
fetch dependencies.

Acceptance requires real executable-shebang tests, argument fidelity (spaces,
empty and non-UTF-8 arguments), status propagation, original-source diagnostics,
stdlib/path/Git dependencies, offline/locked failures, read-only directories,
symlink launchers, surrounding-workspace independence, concurrent launches,
and invalidation after source/dependency/compiler changes. Verify that warm
runs skip compilation and linking, and failed builds never run an old binary.

Before implementation, settle the storage location/lifetime, exact header
grammar, and whether first delivery includes durable lockfiles or restricts
reproducible sharing to ordinary packages. A single-file test command needs
its own entry-point/discovery design: existing package tests reject a test
file mixing `main` and `test_` functions. It is not implicitly solved here.
Scheduling remains in B17.
