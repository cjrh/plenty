# Releasing Plenty

A release is one `vX.Y.Z` tag on `main`. The maintainer creates and pushes it
with [cargo-release](https://github.com/crate-ci/cargo-release); the tag push
starts `.github/workflows/release.yml`, which builds, tests, and publishes the
GitHub Release. Users install it as described in
[installing a release](../design/toolchain/01-building/installing.md).

Only the maintainer releases, after the changes to ship have been reviewed and
merged under the [local PR policy](workflow.md). Agents and PR branches never
run `cargo release --execute`, create release tags, or push.

## Versions

| Package | Version policy |
|---|---|
| `plenty` | The language release version. `cargo release` bumps it in `Cargo.toml` and `Cargo.lock` |
| `plenty-runtime` | Private and embedded in the compiler. Keeps its own version; `plenty-runtime/release.toml` excludes it, so it is never bumped or tagged |
| `plenty-dataframe-engine` | Separately built workspace; its crate version is internal. Release archives bundle it under the Plenty version on Linux, macOS ARM64 and Windows x86-64, with the ABI generation in the shared-library filename |
| `plenty-linalg-engine` | Separately built faer workspace with the same release-version and ABI-generation policy |

`plenty --version` reports the `plenty` package version. Tags are `vX.Y.Z`, or
`vX.Y.Z-PRE` for a pre-release; asset names use the same version.

The root `Cargo.toml` is the source for the current compiler version. The CLI,
LSP server information, source-cache paths, compilation fingerprints, and the
minimum compiler version written by `plenty new`/`plenty init` use Cargo's
`CARGO_PKG_VERSION` at build time. Release scripts read the same manifest to
name archives and fill the packaged README; packaging rejects a compiler whose
reported version disagrees. The Python engine packager reads `[package].version`
directly with `tomllib`, without invoking Bash with a native Windows path;
`check-version.sh` remains the shell interface for archive/tag checks.
Rebuild executables after bumping the manifest.

Do not globally replace older version strings. The private runtime has an
independent version; new user packages start at `0.1.0`; example manifests,
minimum-version requirements, test fixtures, dependency pins, and historical
documentation references describe their own versions. Compiler-output examples
use `X.Y.Z` placeholders so they do not require edits on every release.

The [documentation release catalog](documentation-versions.md#admit-a-published-release)
is separate from a version bump. Admit a version only after its release is
published, preserving earlier archive pins. Development labels identify their
source revision automatically; archived labels come from the selected release.

## Tools

`release.toml` was tested with cargo-release 1.1.6:

```sh
cargo install cargo-release --version 1.1.6 --locked
```

The release build itself needs no local tools: it runs on GitHub Actions. To
rehearse it, Docker is enough (see [checking locally](#checking-locally)).

## Prepare a clean release checkout

cargo-release refuses a dirty tree and commits with `git commit -a`. The
primary checkout often has a modified `issues.db`, the shared issue register
described in `CLAUDE.md`, which must not be reset, stashed, or committed to
make a release. Release from a separate clone of `main` instead, whose
`origin` is the primary checkout's `origin`:

```sh
PRIMARY=/path/to/plenty            # the primary checkout
git clone --branch main "$PRIMARY" ~/plenty-release
cd ~/plenty-release
git remote set-url origin "$(git -C "$PRIMARY" remote get-url origin)"
git fetch origin
git status --short --branch    # must be clean and level with origin/main
```

Bring `main` level with `origin/main` first, through the usual review flow, so
the release push is a fast-forward. Before releasing, run the full checks on
that commit:

```sh
just release-check
```

This runs every Linux CI step, both optional engines, the Docker reverse-proxy
tests, license notices for every release target, the documentation checks, and
`scripts/release/local-check.sh --test`. It continues past a failing step,
writes each step's log to `target/local-ci/`, and prints a summary.
`scripts/local_ci.sh --help` lists the steps; `just ci STEP...` reruns selected
ones. The macOS and Windows release jobs, and the host's other Linux
architecture, still run only in CI.

## Release

Preview, then execute:

```sh
cargo release patch                # dry run: prints the bump, commit, tag, push
cargo release patch --execute      # bump, commit "Release Plenty X.Y.Z", tag vX.Y.Z, push
```

| Level | Example from 0.3.0 |
|---|---|
| `patch` | 0.3.1 |
| `minor` | 0.4.0 |
| `major` | 1.0.0 |
| `rc`, `beta`, `alpha` | 0.3.1-rc.1, then 0.3.1-rc.2 |
| `release` | Drop the pre-release: 0.3.1-rc.2 becomes 0.3.1 |
| `X.Y.Z` | Exactly that version |

The dry run changes nothing.
The configuration rejects branches other than `main`, never publishes to
crates.io, and pushes the commit and tag together (`git push --atomic`).

Afterwards, bring the release commit into the primary checkout without touching
`issues.db`:

```sh
cd "$PRIMARY"
git fetch ~/plenty-release main --tags
git merge --ff-only FETCH_HEAD
```

Then publish the matching [coding-agent skill](agent-skill.md) to
[`plentylang/skills`](https://github.com/plentylang/skills), tagged with the
same version:

```sh
git clone git@github.com:plentylang/skills.git ~/plenty-skills   # once
cd "$PRIMARY"
just skill-sync ~/plenty-skills
cd ~/plenty-skills
git add -A
git commit -m "Plenty X.Y.Z"
git tag vX.Y.Z
git push --atomic origin main vX.Y.Z
```

The [v0.8.9 glibc verification](glibc-compatibility.md) records downloaded-artifact
symbol audits and glibc 2.28 execution evidence for both Linux architectures.

## What the workflow does

Release jobs restore isolated dependency caches. Native compiler and engine
caches share keys with default-branch qualification jobs, including the pinned
toolchain, target flags and manifest hashes. Every release still compiles its
own workspace sources and validates the resulting artifacts. Compiler and engine
target directories do not overlap, so compiler-cache cleanup cannot discard
engine-only dependencies.

Linux host and glibc-image full suites use the existing `ci` Cargo profile:
workspace code is optimized at level 1 while debug assertions and overflow checks
remain enabled. Shipped executables still use the unchanged `release` profile,
CPU baseline and glibc 2.28 environment. The native unit checks retain their
existing profile.

Linux release caches are separate from ordinary Ubuntu CI because the build
images and flags differ. GitHub cache scope lets tag runs restore default-branch
caches, but a cache saved for one tag is unavailable to another tag. A manual
Linux dry run on `main` can seed the Linux caches for subsequent tags; without
that rehearsal they start cold. Native caches are populated by normal
qualification on `main`. Hosted cold/warm timings must be compared after these
changes run; the cache configuration alone is not a measured speedup.

For Linux CI setup, `build.sh --out-dir DIR --test --phase setup` installs and
checks the pinned tools without compiling or packaging. The complete invocation
repeats those checks and runs the suite. `--engine-workspace-targets` places each
engine's Cargo output in its own workspace `target/`, outside the compiler's
`CARGO_TARGET_DIR`; omitting it preserves the local output layout.

| Job | Work |
|---|---|
| Verify tag | Requires the tagged commit to be on `main` and the tag to equal `v` plus the `Cargo.toml` version (`scripts/release/check-version.sh`) |
| Test | The full test suite and release-script tests on native Ubuntu x86-64 and ARM64 runners, with the pinned Rust |
| Build and test | For each architecture in its pinned glibc 2.28 image: installs the pinned Rust, builds both executables with `--locked`, runs the test suite, audits the compiler, then builds/audits both engines and packages them in the compiler archive |
| Smoke test | For each target and its pinned distribution images: installs only the documented prerequisites, disconnects the network, and exercises all archives, including app-only engine relocation with the compiler/original engine removed (`smoke-in-container.sh`) |
| Prepare compiler (experimental) | On `macos-15` and `windows-2025` with the pinned Rust: builds both executables and runs platform-boundary and compiler/runtime unit tests (`build-native.sh --test --phase prepare`) |
| Prepare dataframe (experimental) | Concurrently builds the dataframe engine and runs its formatting, tests and Clippy checks with the pinned baseline |
| Package (experimental) | Requires both preparations, audits the supplied executables and engine, and packages the archive (`build-native.sh --phase package`) |
| Smoke test (experimental) | On a fresh runner of the same kind, with Rust, Cargo and `PLENTY*` variables removed from the environment: checks the archive, then type-checks, compiles and runs a program from paths with spaces, before and after moving the installation (`smoke-native.py`) |
| Publish | Tag pushes only. Creates a draft release with every target's archives and checksums, then publishes it (`publish.sh`) |

The separate [native OS qualification workflow](ci-workflow.md#native-os-qualification)
prepares the compiler and dataframe engine concurrently, then packages their
artifacts on a fresh runner. `build-native.sh --out-dir DIR --test --phase prepare`
only builds/tests the pinned compiler. `--phase package --engine-dir DIR`
packages the prepared compiler binaries from `CARGO_TARGET_DIR/release/` with
previously validated engines from that directory, retaining all packaging
audits. Package mode requires `--engine-dir` and rejects `--test`; the caller
must ensure both preparations succeeded. Omitting `--phase` keeps the complete
local build, engine validation and packaging flow; release jobs select the
explicit phases.

Native Ubuntu test runners and the x86-64 baseline build install the
SHA-256-verified mold version pinned in `scripts/release/toolchain.env`, without
requiring root. They check a linked program's `.comment` section before running
the suite, so a missing linker fails early rather than silently changing
executable sizes. The ARM glibc 2.28 build and its local rehearsal use the GNU
linker already pinned by the build image: upstream's ARM mold 3.0.0 binary
imports `gettid@GLIBC_2.30` and cannot run in that image. This preserves the
existing glibc floor. Size budgets measured with mold require care when comparing
GNU ld outputs, which can retain additional unused runtime code and panic strings.
The clean distribution smoke tests also exercise system linkers; mold is not a
release-installation dependency.

Builds use the tagged source, not moving `main`, a host build, or a cache from
another environment. Publication waits for every job to pass. Pre-release tags
(`vX.Y.Z-PRE`) publish GitHub pre-releases that are not marked latest. Runs for the same tag
are serialized. Only the publish job can write repository contents, through
the workflow token; no crates.io or other credential is used.

`scripts/release/target.sh` defines each target and `scripts/release/matrix.py`
turns the list into job matrices:

| Target | Runner | Archives |
|---|---|---|
| `x86_64-unknown-linux-gnu` | `ubuntu-24.04` | compiler with dataframe and linalg engines (`.tar.gz`) |
| `aarch64-unknown-linux-gnu` | `ubuntu-24.04-arm` | compiler with dataframe and linalg engines (`.tar.gz`) |
| `aarch64-apple-darwin` (experimental) | `macos-15` | compiler with dataframe dylib (`.tar.gz`), macOS 11 or later |
| `x86_64-pc-windows-msvc` (experimental) | `windows-2025` | compiler with dataframe DLL (`.zip`), Windows 10 or later |

Each target publishes one asset, `plenty-VERSION-TRIPLE.EXT`, and its checksum.
`release_engines` in `target.sh` lists the engines bundled for a target. Linux bundles both ELF engines; Windows bundles the dataframe DLL, built and
validated natively by `build-native.sh`. macOS bundles the dataframe dylib,
also built and validated natively, with a macOS 11 deployment minimum,
`@rpath/libplenty_dataframe_v1.dylib` identity, system-only dependencies and
verified ad-hoc signature. Native archive modes distinguish the two compiler
executables (0755) from engine libraries and notices (0644).
Windows packaging audits x86-64 PE headers, Windows/MSVC dependencies and all
facade exports. The native clean smoke tests check a Parquet query, compiler relocation,
`--dist` application relocation and failure without the engine. Native archives are
not notarized or signed with a distribution identity; macOS executables and
the dataframe dylib carry linker-generated ad-hoc signatures, and the Windows build links the dynamic MSVC runtime that the
documented Build Tools prerequisite installs. `archive.py` writes those archives
with sorted entries, fixed owners and modes, and `SOURCE_DATE_EPOCH` timestamps.
The release notes mark them experimental. The [ARM harness](linux-aarch64.md)
records the emulated evidence gathered before native ARM runners.
A Linux archive contains `bin/libplenty_NAME_v1.so` beside the compiler and
`bin/NAME-notices/` with licenses, dependency notices and `engine.json`
identifying the release/source, target and library checksum. `build.sh` builds
each engine and `package.sh --engine NAME=LIBRARY` installs it through
`package-engine.py`; the archive README covers using and shipping the engines.
`package-engine.py NAME` rejects an incorrect SONAME, embedded search paths,
unsupported native dependencies/symbol versions, and missing symbols required
by that checkout's stdlib facade. This catches stale engines with the same
ABI-generation name. The workflow builds all components from the tagged
source; it never substitutes an engine downloaded from another release.

Normal compiler builds remain independent of Polars and faer, and the compiler
links neither; programs load an engine only when they reach it.
The release engine is built once for reuse when compiling and running many
applications. The engine's native dependency graph supplies its own notices;
it is not a copy of the compiler's dependency inventory.

### The release build environment

`scripts/release/toolchain.env` pins every input that is not in the lockfile:

- `BUILD_IMAGE` and `BUILD_IMAGE_AARCH64`: architecture-matched
  `quay.io/pypa/manylinux_2_28_x86_64` and `manylinux_2_28_aarch64` images by
  digest. They provide AlmaLinux 8 with glibc 2.28 and gcc-toolset, so C/Rust
  objects are built against that baseline. ELF symbol auditing and execution
  in the oldest smoke environment must confirm the result for each target.
- `RUST_VERSION`, architecture-specific checksummed `rustup-init`, and mold.
- `SMOKE_IMAGES` and `SMOKE_IMAGES_AARCH64`: separate pinned inventories.
  x86 retains its six distributions; ARM uses AlmaLinux 8 and Debian 13.
  A distribution without a matching ARM image is not silently treated as tested.

Update these deliberately: change the pin, run `scripts/release/local-check.sh`,
and update the tested distribution list in `scripts/release/README.dist.md`
and the installation page when it changes. `check-elf.sh` fails the build if a
change raises the glibc requirement.

## Recovering from a failed release

Never move or delete a pushed release tag, and never change a published
release.

- **A job failed before publishing**, for example a flaky test or an
  unavailable image: rerun the failed jobs. No release exists yet, or only a
  draft.
- **Publishing was interrupted**: rerun the publish job. `publish.sh` finds
  the draft, replaces its assets, and publishes it, without creating a second
  release.
- **A rerun after publishing**: succeeds without changes when both published
  checksums match, and fails otherwise, including a missing engine checksum.
- **The tagged source is wrong**: fix it on `main` through review and release
  the next version. Delete an unpublished draft for the bad tag in the GitHub
  interface if one remains; the tag stays.

## Dry runs

Run the workflow manually to test it before tagging. A manual run builds and
smoke-tests the selected targets from the chosen branch and never publishes;
the publish job runs only for tag pushes. It skips the tag checks and reads the
version from `Cargo.toml`.

```sh
gh workflow run release.yml --ref BRANCH -f targets=all    # or linux, native
gh run watch
```

Or use **Actions → Release → Run workflow** in the GitHub interface. The
built archives remain downloadable from the run's artifacts for 30 days.

## Checking locally

`scripts/release/local-check.sh` runs the build and smoke jobs with Docker,
without tagging, uploading, or publishing. It builds in the pinned image into
`target/release-container-ARCH/`, separate from host builds, and smoke-tests the
archives in that target's pinned distributions. By default it selects the host
architecture; `PLENTY_RELEASE_TARGET` explicitly selects one of the two triples.
The native release build refuses a mismatched host rather than quietly relying
on emulation. For example, on an ARM host:

```sh
PLENTY_RELEASE_TARGET=aarch64-unknown-linux-gnu scripts/release/local-check.sh --test
```

The engine builder accepts `--target TRIPLE` and `--validate`; a successful
cross-build alone does not validate engine execution. Options:

| Option | Effect |
|---|---|
| `--test` | Also run the full test suite in the build image |
| `--skip-smoke` | Only build, audit, and package |

`scripts/release/smoke-test.sh ARCHIVE` runs the archive checks on the host.
`bash scripts/release/smoke-engine.sh ARCHIVE dataframe` checks the bundled
engine's files and modes, optional linkage, discovery beside the compiler, a
Parquet query, `--dist` relocation without the compiler, and the missing runtime
library diagnostic. `smoke-engine.sh ARCHIVE linalg` runs the same checks with
a linear solve. `smoke-in-container.sh` runs both after `smoke-test.sh`.
Containers are labelled `org.plenty.release-build` and
`org.plenty.release-smoke` and removed when the check ends.

## Licenses in the archive

See [release archive licenses](release-licenses.md) for inventories and
maintenance requirements.
