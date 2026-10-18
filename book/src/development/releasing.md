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

`plenty --version` reports the `plenty` package version. Tags are `vX.Y.Z`, or
`vX.Y.Z-PRE` for a pre-release; asset names use the same version.

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

## What the workflow does

| Job | Work |
|---|---|
| Verify tag | Requires the tagged commit to be on `main` and the tag to equal `v` plus the `Cargo.toml` version (`scripts/release/check-version.sh`) |
| Test | The full test suite and release-script tests on the Ubuntu host, with the pinned Rust |
| Build and test | In the pinned glibc 2.28 image: installs the pinned Rust, builds both executables with `--locked`, runs the test suite with that toolchain, rejects binaries that need more than glibc 2.28 (`check-elf.sh`), and packages the archive (`package.sh`) |
| Smoke test | For each pinned distribution image: installs only the documented prerequisites, disconnects the network, and runs `smoke-test.sh` on the archive (`smoke-in-container.sh`) |
| Publish | Creates a draft release with the archive and checksum, then publishes it (`publish.sh`) |

The build never uses moving `main`, a host build, or a cache from another
environment. The release is published only after every job passes, so a failed
build never appears as a finished release. Pre-release tags (`vX.Y.Z-PRE`)
publish GitHub pre-releases that are not marked latest. Runs for the same tag
are serialized. Only the publish job can write repository contents, through
the workflow token; no crates.io or other credential is used.

### The release build environment

`scripts/release/toolchain.env` pins every input that is not in the lockfile:

- `BUILD_IMAGE`: `quay.io/pypa/manylinux_2_28_x86_64` by digest. AlmaLinux 8
  with glibc 2.28 and gcc-toolset, maintained by the PyPA for exactly this
  purpose. Building there fixes the glibc baseline of the compiler and of the
  embedded runtime archives, whose C and Rust objects are compiled against
  glibc 2.28 headers.
- `RUST_VERSION` and a checksummed `rustup-init`.
- `SMOKE_IMAGES`: the distributions the README names as tested.

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
- **A rerun after publishing**: succeeds without changes when the published
  checksum matches, and fails otherwise.
- **The tagged source is wrong**: fix it on `main` through review and release
  the next version. Delete an unpublished draft for the bad tag in the GitHub
  interface if one remains; the tag stays.

## Checking locally

`scripts/release/local-check.sh` runs the build and smoke jobs with Docker,
without tagging, uploading, or publishing. It builds in the pinned image into
`target/release-container/`, separate from host builds, and smoke-tests the
archive in every pinned distribution. Options:

| Option | Effect |
|---|---|
| `--test` | Also run the full test suite in the build image |
| `--skip-smoke` | Only build, audit, and package |

`scripts/release/smoke-test.sh ARCHIVE` runs the archive checks on the host.
Containers are labelled `org.plenty.release-build` and
`org.plenty.release-smoke` and removed when the check ends.

## Licenses in the archive

Plenty is licensed MIT OR Apache-2.0 (`license` in both manifests). The
archive ships `LICENSE-MIT` and `LICENSE-APACHE` from the repository root and
`THIRD-PARTY-NOTICES.md`, generated by `scripts/release/third_party_notices.py`
from `cargo metadata`. It lists every crate linked into the compiler or its
embedded runtime (normal dependencies only; build scripts and procedural macros
do not ship) with each crate's own license files, plus the vendored SQLite
amalgamation. The Rust standard library's notices come from the build
toolchain's `share/doc/rust`. The script fails when a shipped crate has no
license file, so a new dependency's license is reviewed before it ships.
