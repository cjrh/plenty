# Single-file scripts

`plenty script` compiles a standalone application and runs it without creating a
package. It requires the normal native compiler/linker environment and an explicit
[`main`](../../language/02-source/entrypoint.md), not top-level execution.

```plenty
#!/usr/bin/env -S plenty script

def main() -> Result[(), IoError]:
    print("Hello, Plenty!")?
    Ok(())
```
```output
Hello, Plenty!
```

Save it as `hello.plenty` and run:

```sh
plenty script hello.plenty 123
chmod +x hello.plenty
./hello.plenty 123
```

## Arguments and modes

```text
plenty script [OPTIONS] [--] FILE [ARGS...]
plenty script --check hello.plenty
plenty script --compile -o ./hello hello.plenty
```

Runner options precede `FILE`. Everything after it is passed unchanged to the
application, including flags, empty strings and a literal `--`. `--` before `FILE`
allows filenames beginning with `-`. Thus `plenty script --help` displays runner
help, while `./hello.plenty --help` asks the application for help.

On Unix, [`args()`](../../facilities/02-io/streams-args.md#arguments) starts with
the invoked script path, including a symlink launcher's spelling;
[`args_bytes()`](../../facilities/02-io/bytes-paths.md) preserves native argument
bytes without UTF-8 conversion. Retained binaries have normal executable argument
zero. Streams, environment and working directory are inherited; imports do not
change the working directory. Child exit codes and Unix signal status follow
[ordinary compile-and-run](commands.md).

`--check` and `--compile -o OUT` resolve the same dependencies and preserve source
diagnostics, but reject program arguments and never run the application.
Scripts optimize by default; `--fast-compile` skips optimization for shorter
edit-run cycles. `--release` remains a compatibility alias for the default.
`--offline`, `--locked`,
`--verbose`, `--no-strip` and `--no-cache` retain their ordinary meanings.
`--cache-stats` reports artifact reuse. Build status goes to stderr; a fresh cached
run is quiet unless reporting was requested.

## Optional embedded manifest

A script may have one `---plenty` line, TOML metadata, and one closing `---` line
before its declarations. The optional shebang must be the first line; blank lines
may precede the header. Delimiters are unindented lines with no extra whitespace.
Duplicate, misplaced and unterminated headers are errors. CRLF is supported.
Imported modules cannot contain script manifests. `plenty FILE` and `plentyc`
remain compiler interfaces: an embedded header directs their users here rather
than fetching dependencies.

For example, given an existing library package at `../util`:

```text
#!/usr/bin/env -S plenty script
---plenty
[package]
name = "helper"

[dependencies]
util = { path = "../util", version = "^0.1.0" }
---
from util import answer

def main() -> Result[(), IoError]:
    print(answer())?
    Ok(())
```

The [package manifest](packages.md) dependency schema applies, including Git
`tag`/`rev`, `subdir`, and version requirements. No registry or version-only lookup
is introduced. `manifest-version` defaults to 1; `[package]` is optional, with
version `0.0.0` and a name derived from the lowercase filename stem. Invalid name
characters become `_`; names needing an initial identifier character or colliding
with reserved `std` receive a `script_` prefix. Explicit names use package
validation. Description, authors, license and minimum `plenty-version` are
supported. Unknown keys, workspaces and explicit library/binary targets are rejected.
There is exactly one target: the original script. Neighboring targets and enclosing
workspaces are never discovered.

The physical script directory is both the module root and the base of relative
dependencies. A symlink launcher uses resources beside its target. Sibling modules
are supported, with normal reserved-name, alias-conflict and cycle checks. A script
with sibling modules or path dependencies is not self-contained when copied.
Diagnostics retain original filenames, lines and body byte positions. Editors
recognize the header; dependency completion uses already cached resolution without
fetching. A script run with only an explicit durable lock may need an ordinary
cached-lock run before editor dependency resolution is available.

## Storage and locks

Generated dependency state lives under `scripts/state/HASH/` in `PLENTY_CACHE_DIR`,
or `$XDG_CACHE_HOME/plenty`, or `~/.cache/plenty`. `HASH` covers the canonical script
path, not its basename. Unix directories are created private to the user. Nothing
is generated beside the source by default, so read-only script directories work.
Per-script file locks serialize resolution; locks are released before running the
application. Successful object and executable entries are published atomically;
runs execute private copies, never a shared writable binary. Failed builds do not
run an earlier executable.

Dependencies use a cached `plenty.lock`, retained across body edits and reconciled
when dependencies change. `--locked` rejects missing or stale locks; `--offline`
uses cached dependency sources only. A dependency-free first run needs no lock and
works with both flags. Artifact caching can be disabled without disabling the
persistent dependency state needed by these flags.

For a durable, shareable lock:

```sh
plenty script --lockfile ./hello.lock hello.plenty
plenty script --locked --offline --lockfile ./hello.lock hello.plenty
```

The option path is relative to the caller and must be outside the Plenty cache
so cleanup cannot remove it; local source paths inside the lock are relative to
the physical script directory. Cached locks are not portable
reproducibility guarantees: clearing the cache or moving a script can resolve a
moved tag differently. Full revisions do not pin mutable tags in transitive
dependencies; path dependencies remain live trees.

`plenty cache stats` includes script dependency state, and `plenty cache clean`
removes it as well as compiled artifacts. Cleanup takes each namespace's lock;
small coordination lock files remain so concurrent launches keep using the same
lock. Automatic artifact-size trimming does not evict dependency state. Durable
locks outside managed cache state are not removed. Executable reuse has the
[existing Linux toolchain eligibility limits](../02-architecture/cache.md#single-file-executables);
other toolchains link normally. Unix shebang/argument behavior is tested on Linux;
non-Unix script argument zero is the generated executable name.

This is not a sandbox or interpreter, and does not provide a script test command.
For a multi-target application, portable package locks, or package test discovery,
use [packages](packages.md). See the [design rationale](../../../proposals/completed/single-file-scripts.md)
for alternatives.
