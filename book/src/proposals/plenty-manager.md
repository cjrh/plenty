# Plenty toolchain manager

> **Status: Not implemented.** Proposed under [B17](../backlog.md).
> The tool is named `plenty-manager`, written in Plenty, and intended for a
> new, separate repository named `plenty-manager` in the `plentylang`
> organization on GitHub. This proposal does not create that repository.
> Commands, configuration and storage below are proposed interfaces.

## Motivation and existing behavior

Installing, upgrading and switching compilers should not require editing shell
startup files for every release. A project should be able to commit the exact
compiler version used by its developers and CI, while older projects keep their
own versions.

Today, [release archives](../design/toolchain/01-building/installing.md) are
relocatable and can coexist in versioned directories. The first compiler on
`PATH` runs. The compiler embeds ordinary standard-library sources and runtime
code; Linux archives also carry the native engines in `bin/`. These properties make a
manager possible without changing how compilation works.

The package manifest's
[`plenty-version`](../design/toolchain/01-building/packages.md#manifest) is a
minimum supported compiler version, not a selection instruction. Preserve that
meaning. Dependency locking, compiler selection and proposed
[language editions](language-editions.md) solve different problems.

[Rustup's toolchain selection](https://rust-lang.github.io/rustup/overrides.html)
is the precedent: executable proxies select an installation before Cargo or the
compiler runs. Adopt the small useful subset, not its complete component,
channel or override system.

## Ownership and implementation

Implement the manager and its executable proxies in Plenty in the separate
`plentylang/plenty-manager` repository. Keep compiler release production and
archive compatibility in the compiler repository. Agree on a versioned release
metadata contract between them; neither repository should need to import the
other's implementation.

Distribute native manager executables, so installing Plenty does not first
require Plenty, Rust, Cargo or a source checkout. Maintainers build the manager
with an explicitly pinned, already published Plenty compiler. Retain documented
manual archive installation as the bootstrap and recovery path. A manager
release must not require the compiler release it is intended to install.

Use Plenty's existing process, filesystem, HTTP/TLS, hashing and configuration
facilities where suitable. Audit the required APIs and platform behavior before
implementation; this proposal is not evidence that secure extraction, file
locking or executable replacement already has every needed primitive. Keep
manageable application logic in Plenty. Security-sensitive verification may
use existing audited libraries through standard-library facilities rather than
new cryptographic implementations. Record discovered compiler/library defects
in the issues register, not as silent installer workarounds.

## First user-facing interface

Illustrative versions below are placeholders, not published release claims.

```text
plenty-manager install X.Y.Z
plenty-manager list
plenty-manager default X.Y.Z
plenty-manager show
plenty-manager run X.Y.Z -- plenty build
plenty-manager remove X.Y.Z
plenty-manager update
```

`install` fetches an exact version for the native host and leaves all existing
versions intact. It does not change an existing default. On a fresh installation,
setting the default is an explicit step. `list` shows installed versions and the
default; `show` reports the effective version, selection source and executable
path for the current directory, including a missing selected installation.

`run` explicitly selects an installed version for `plenty` or `plentyc`, with
remaining arguments forwarded literally. It does not invoke a shell or search
`PATH` for the selected compiler. Unknown tool names are rejected.

`update` explicitly discovers and installs the newest stable compiler supported
on the host, then makes it the user default. It reports the resolved exact
version, preserves previous installations, and never edits project pins. It
is the only initial interface that resolves a moving release name. Interrupted
or failed updates leave the old default working. Manager self-update is not
part of this command or the initial scope.

`remove` rejects removal of the current default until another default is set.
It cannot discover every project pin: warn about that limitation and report a
clear missing-toolchain error when such a project is later used. Removal must
not damage running tools; platforms that cannot remove an in-use installation
must fail safely or defer cleanup.

## Project pin and selection

Use a separate, committed file rather than overloading the library's minimum
compatibility requirement:

```toml
# plenty-toolchain.toml — proposed
[toolchain]
version = "X.Y.Z"
```

The initial schema accepts one exact published `MAJOR.MINOR.PATCH` version.
Reject ranges, moving channels, paths, unknown keys and malformed files rather
than guessing. Pre-release channels and local development toolchains are
separate extensions, not implicit initial behavior.

A proxy selects, in order:

1. An explicit `plenty-manager run VERSION -- TOOL ...` selection.
2. `PLENTY_TOOLCHAIN`, containing an exact version.
3. The nearest `plenty-toolchain.toml`, searching from the caller's current
   directory upward to the filesystem root.
4. The user's configured default.

Invalid higher-priority selections are errors, not reasons to fall back. With
no selection, explain how to install and select a default. With a selection
that is not installed, show the exact `plenty-manager install VERSION` command.
Neither proxy invocation nor a project file causes a network request.

Discovery deliberately follows the working directory, not a source filename or
`--manifest-path`. For a project addressed from elsewhere, enter its directory
or use an explicit selection. Workspace members normally share a root pin;
a nearer member pin overrides it when invoked there. A single compilation
still uses one compiler for the complete dependency graph. Dependency manifests
and pins never switch that compiler during resolution.

A newer compiler remains free to reject a package for other compatibility
reasons. The selected compiler continues to enforce every package's minimum
`plenty-version`; the manager does not solve a second dependency graph or
silently select a newer compiler to satisfy it.

## Proxy and installation layout

Add one per-user manager `bin` directory to `PATH` during explicit setup. It
contains `plenty-manager` and proxies named `plenty` and `plentyc` (with native
executable suffixes where required). Managed releases live outside that
namespace, keyed by exact version and native host triple. The proxy invokes
the absolute compiler path, never itself through `PATH`.

Choose platform-appropriate user data/configuration directories, with an
explicit `PLENTY_MANAGER_HOME` override for CI and portable installations.
Keep downloaded staging files distinct from complete installations. Do not
write manager state inside project checkouts, require root, or modify shell
startup files without consent. Keep archive-based installations usable outside
the manager.

Forward arguments, working directory, environment, streams and process results
without reinterpretation. On Unix, prefer process replacement for transparent
signal handling; where a supervising process is necessary, preserve status,
interruption and child lifetime. `--version` is handled by the selected compiler;
`plenty-manager show` explains the dispatch. Preserve the explicit selection in
child environments so nested tool invocations cannot accidentally switch back
to a project pin or default.

Install the compiler, embedded runtime and standard library as one release.
Optional native engines must match the release and host compatibility contract;
do not reuse an arbitrary engine from another installation. Initial delivery
may omit engine management, but must say so and retain documented manual engine
installation. The manager does not install system linkers, SDKs, libc development
files or unsupported cross-compilation targets.

## Download trust, persistence and failure behavior

Download only from the configured official distribution endpoint over verified
TLS. Release metadata must identify its schema version, exact compiler version,
host triple, archive URL, size and cryptographic digest. Resolve moving updates
to that immutable identity before installation. Reject unsupported hosts before
downloading; available compiler archives do not prove that the Plenty-written
manager is qualified on the same platform.

A checksum served beside an archive detects corruption but is not independent
authentication. Before shipping automatic installation, settle authenticated
release metadata or signed artifacts, trust-root distribution and rotation,
and downgrade/replay policy. Do not describe HTTPS plus adjacent checksums as
a complete supply-chain security design. Use reviewed verification primitives.

Bound metadata and download sizes. Treat archives as untrusted: reject path
traversal, absolute paths, escaping links, duplicate/conflicting entries and
unexpected file types; validate the expected layout before publication. Never
execute downloaded hooks. Avoid shell construction from versions, filenames or
URLs, and do not execute the installed compiler as a substitute for verification.

Use private staging directories, per-installation locking and atomic publication
on the destination filesystem. Update defaults atomically only after successful
installation. Concurrent installs must agree on one complete result; failed
verification, disk exhaustion or interruption must never expose a partial
compiler. Retain enough metadata to identify each installed artifact and reject
conflicting content under an existing release identity. Define safe recovery
for stale locks and staging files without deleting active work.

Dispatch, inspection, removal and execution work offline. Explicit installation
and update fail clearly without network access; no command silently substitutes
another compiler. A compiler pin improves repeatability but does not pin Git
sources, native engines, linker/SDK versions or the system C library. See
[build reproducibility](../design/toolchain/01-building/linking.md).

## Delivery and acceptance

Keep priority and scheduling solely in [B17](../backlog.md#tooling-investigations).
Before implementation, agree on the release-metadata trust contract, supported
manager hosts, storage layout and optional-engine scope. Creating the separate
GitHub repository and publishing binaries are later, explicit operations, not
part of adopting this document.

The first delivery should prove exact-version installation, side-by-side use,
proxies, project pins, explicit overrides, default changes, explicit updates,
removal and recovery. Require integration tests for:

- Two compiler releases used by separate projects without changing `PATH`, and
  manual archive installations still working.
- Selection precedence, malformed/missing pins, workspace/member discovery,
  dependency-pin isolation and calls from outside a project directory.
- Offline invocation with network access prohibited, including nested tools.
- Argument fidelity, spaces/Unicode in paths, supported native argument bytes,
  exit status, signals and interruption on each admitted host.
- Corrupt downloads, invalid trust metadata, hostile archives, unsupported
  hosts, disk exhaustion, concurrent installation/removal and interrupted update.
- A downloaded manager installing its build compiler from a clean machine
  without Plenty or Rust already installed, plus manual recovery without it.
- Optional engine/version isolation if engine management is included.

Document platform qualification, limitations and the compiler version used to
build each manager release. Defer nightly/beta channels, target/component
matrices, directory-override databases, automatic fetching on dispatch,
self-update, arbitrary mirrors and automatic project-pin rewriting. A separate
manager keeps these concerns out of the compiler while preserving a useful,
small initial tool.
