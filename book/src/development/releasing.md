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
| `plenty-dataframe-engine` | Optional separately built workspace; its crate version is internal. Release archives use the Plenty version and carry the ABI generation in the shared-library filename |
| `plenty-linalg-engine` | Optional faer workspace with the same release-version and ABI-generation policy |

`plenty --version` reports the `plenty` package version. Tags are `vX.Y.Z`, or
`vX.Y.Z-PRE` for a pre-release; asset names use the same version.

The root `Cargo.toml` is the source for the current compiler version. The CLI,
LSP server information, source-cache paths, compilation fingerprints, and the
minimum compiler version written by `plenty new`/`plenty init` use Cargo's
`CARGO_PKG_VERSION` at build time. Release scripts read the same manifest to
name archives and fill the packaged README; packaging rejects a compiler whose
reported version disagrees. Rebuild executables after bumping the manifest.

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
cargo test --locked --workspace --all-features
cargo clippy --locked --workspace --all-targets -- -D warnings
scripts/release/local-check.sh
```

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

The dry run changes nothing: no manifest edits, commits, tags, or pushes.
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

## What the workflow does

| Job | Work |
|---|---|
| Verify tag | Requires the tagged commit to be on `main` and the tag to equal `v` plus the `Cargo.toml` version (`scripts/release/check-version.sh`) |
| Test | The full test suite and release-script tests on native Ubuntu x86-64 and ARM64 runners, with the pinned Rust |
| Build and test | For each architecture in its pinned glibc 2.28 image: installs the pinned Rust, builds both executables with `--locked`, runs the test suite, audits the compiler, then builds/audits both optional engines and packages separate archives |
| Smoke test | For each target and its pinned distribution images: installs only the documented prerequisites, disconnects the network, and exercises all archives, including app-only engine relocation with the compiler/original engine removed (`smoke-in-container.sh`) |
| Build and test (experimental) | On `macos-15` and `windows-2025` with the pinned Rust: builds both executables, runs the platform-boundary and compiler/runtime unit tests, audits the executables (`check-native.py`), and packages the core archive (`build-native.sh`) |
| Smoke test (experimental) | On a fresh runner of the same kind, with Rust, Cargo and `PLENTY*` variables removed from the environment: checks the archive, then type-checks, compiles and runs a program from paths with spaces, before and after moving the installation (`smoke-native.py`) |
| Publish | Tag pushes only. Creates a draft release with every target's archives and checksums, then publishes it (`publish.sh`) |

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

The build never uses moving `main`, a host build, or a cache from another
environment. The release is published only after every job passes, so a failed
build never appears as a finished release. Pre-release tags (`vX.Y.Z-PRE`)
publish GitHub pre-releases that are not marked latest. Runs for the same tag
are serialized. Only the publish job can write repository contents, through
the workflow token; no crates.io or other credential is used.

`scripts/release/target.sh` defines each target and `scripts/release/matrix.py`
turns the list into job matrices:

| Target | Runner | Archives |
|---|---|---|
| `x86_64-unknown-linux-gnu` | `ubuntu-24.04` | compiler, dataframe and linalg engines (`.tar.gz`) |
| `aarch64-unknown-linux-gnu` | `ubuntu-24.04-arm` | compiler, dataframe and linalg engines (`.tar.gz`) |
| `aarch64-apple-darwin` (experimental) | `macos-15` | compiler only (`.tar.gz`), macOS 11 or later |
| `x86_64-pc-windows-msvc` (experimental) | `windows-2025` | compiler only (`.zip`), Windows 10 or later |

Compiler assets are `plenty-VERSION-TRIPLE.EXT`; optional engine assets are
`plenty-dataframe-VERSION-TRIPLE.tar.gz` and `plenty-linalg-VERSION-TRIPLE.tar.gz`.
An engine must match the compiler's target. The engines are Linux ELF shared
libraries, so macOS and Windows publish only the compiler. Their archives are
not notarized or code-signed; macOS executables carry the linker's ad-hoc
signature, and the Windows build links the dynamic MSVC runtime that the
documented Build Tools prerequisite installs. `archive.py` writes those archives
with sorted entries, fixed owners and modes, and `SOURCE_DATE_EPOCH` timestamps.
The release notes mark them experimental. The [ARM harness](linux-aarch64.md)
records the emulated evidence gathered before native ARM runners.
Each engine archive contains `lib/libplenty_NAME_v1.so`, deployment instructions,
notices and `engine.json` identifying the release/source, target and library
checksum.
`package-engine.py NAME` rejects an incorrect SONAME, embedded search paths,
unsupported native dependencies/symbol versions, and missing symbols required
by that checkout's stdlib facade. This catches stale engines with the same
ABI-generation name. The workflow builds all components from the tagged
source; it never substitutes an engine downloaded from another release.

Normal compiler builds and the compiler download remain independent of Polars and faer.
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

For a host-only compiler/engine installation with `--dist` notices, use the
[just recipes](workflow.md#build-a-complete-local-installation). Their explicit
`--local-install` packaging mode does not produce release archives or enforce
the portable GLIBC/GCC symbol baseline. The release workflow never selects it.

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

Normal CI requires the all-feature suite, doctests and a default-feature build
on both `ubuntu-24.04` and `ubuntu-24.04-arm`; function/build cache keys include
runner architecture. Optional engines are built and exercised independently.
The engine builder accepts `--target TRIPLE` and `--validate`; a successful
cross-build alone does not validate engine execution. Options:

| Option | Effect |
|---|---|
| `--test` | Also run the full test suite in the build image |
| `--skip-smoke` | Only build, audit, and package |

`scripts/release/smoke-test.sh ARCHIVE` runs the archive checks on the host.
`bash scripts/release/smoke-dataframe.sh COMPILER_ARCHIVE ENGINE_ARCHIVE`
checks matching assets, optional linkage, engine discovery, a Parquet query,
relocation without the compiler, and the missing runtime library diagnostic.
`bash scripts/release/smoke-engine.sh COMPILER_ARCHIVE ENGINE_ARCHIVE linalg`
exercises the same installation/relocation checks with a linear solve.
Containers are labelled `org.plenty.release-build` and
`org.plenty.release-smoke` and removed when the check ends.

## Licenses in the archive

Plenty is licensed MIT OR Apache-2.0 (`license` in both manifests). The
archive ships `LICENSE-MIT` and `LICENSE-APACHE` from the repository root and
`THIRD-PARTY-NOTICES.md`, generated by `scripts/release/third_party_notices.py`
from `cargo metadata`. It lists every crate linked into the compiler or its
embedded runtime (normal dependencies only; build scripts and procedural macros
do not ship) with each crate's own license files, plus the vendored SQLite
amalgamation and Brotli codec, and the third-party code and data that standard
library modules and the runtime embed. `VENDORED_SOURCES` lists source copied
from a published package with its license files kept beside the copy:
`plenty-runtime/src/unicode/vendor` holds unicode-normalization 0.1.25
(MIT OR Apache-2.0). `EMBEDDED` lists the rest: `std.markdown` derives from
commonmark.js 0.31.2 (BSD-2-Clause) and carries tables generated from the
Unicode Character Database 16.0.0 and the WHATWG HTML named character
references (BSD-3-Clause for portions in source code), and the
unicode-normalization tables derive from the Unicode Character Database 17.0.0
(Unicode License v3). `std.diff` derives its Myers and patience
algorithms from the `similar` crate 3.2.1 (Apache-2.0). Their reviewed license texts are bundled in
`scripts/release/licenses/`; update the entry and its text together when the
upstream version changes. The compiler notices fail when a directory named
`vendor`, or a subdirectory of `plenty-runtime/native`, has no notice: a
child of a `vendor` directory named after a shipped crate (`NAME-VERSION`, as in
`plenty-runtime/vendor`) is covered by that crate's entry, and any other vendored
directory needs a `VENDORED_SOURCES` or native-library entry. The Rust standard library's notices come from the build
toolchain's `share/doc/rust`. The script fails when a shipped crate has no
license file or reviewed fallback, so a new dependency's license is reviewed
before it ships. For `iced-x86 1.21.0`, whose published crate omits its license
file, the generator uses a bundled copy from the upstream commit recorded in
the crate's `.cargo_vcs_info.json`. The fallback matches the package name,
version, and declared license and includes the source URL in the notices.
`yaxpeax-arm 0.5.0`, the A64 assembly decoder, declares `0BSD` but omits a
license file; its pinned fallback supplies the standard SPDX 0BSD terms without
inventing a copyright notice. Both fallbacks identify their package version and
source URL. Dependency upgrades require a fresh review if they still omit the license file;
notice generation does not download fallback text during the release build.

For each separate engine archive, `third_party_notices.py --component NAME`
walks its own manifest/lockfile. The faer fork and its backend are Git dependencies
pinned in the linalg lockfile, with their upstream notices; missing workspace-root
licenses in the dependency family have version-pinned, reviewed fallbacks.
Offline builds need the pinned Git checkout as well as Cargo's registry cache.
For dataframe, `alloc-stdlib` shares its repository's
BSD license with `alloc-no-stdlib`. `polars-parquet-format 0.1.0` declares
MIT AND Apache-2.0 but omits top-level license files; a version/license-pinned
fallback preserves its upstream `parquet-format-safe` license texts and the
published package's author attribution. Its Thrift sources retain Apache
Software Foundation license headers. The bundled fallback identifies its
upstream revision, and dependency upgrades require review again.
