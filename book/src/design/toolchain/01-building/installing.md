# Installing a release

Each Plenty release on the project's GitHub Releases page provides one archive,
`plenty-VERSION-x86_64-unknown-linux-gnu.tar.gz`, and its `.sha256` checksum.
Extract it anywhere and put its `bin/` directory on `PATH`. No installer, root
access, fixed location, Rust, Cargo, or source checkout is needed. The README
in the archive repeats these instructions.

For a step-by-step download guide and persistent PATH examples for Bash, Zsh,
and Fish, start with [Setting up](../../../tutorial/00-setting-up.md).

```text
plenty-VERSION-x86_64-unknown-linux-gnu/
  bin/plenty                  compiler and project tool
  bin/plentyc                 single-file compiler
  README.md                   requirements and installation
  LICENSE-MIT                 Plenty's license: MIT or Apache-2.0,
  LICENSE-APACHE                at your option
  THIRD-PARTY-NOTICES.md      licenses of software built into the compiler
  licenses/rust/              the Rust standard library's notices
```

The standard library and the runtime are built into the executables, so the
archive has no other support files and works from any directory.

## Requirements

| Requirement | Detail |
|---|---|
| Platform | x86-64 Linux with glibc 2.28 or later: RHEL/AlmaLinux/Rocky 8, Debian 10, Ubuntu 18.10, Fedora 29, or newer |
| Kernel | 3.2 or later, the Rust standard library's minimum; old kernels are not tested |
| CPU | Any x86-64 CPU; the compiler and generated programs use the original x86-64 (SSE2) instruction set |
| Linking | A `cc` driver, linker, startup files, and C library development files |

musl distributions such as Alpine are not supported. Other targets, including
macOS, Windows, and ARM, have no release archive; see
[native linking and targets](linking.md).

Plenty generates machine code itself and runs `cc` only to link it with the
embedded runtime and the system C library, as described in
[native linking](linking.md). It is not a self-contained toolchain. Install the
linking prerequisites:

```sh
# Debian, Ubuntu
sudo apt-get update && sudo apt-get install gcc libc6-dev

# Fedora, RHEL, AlmaLinux, Rocky Linux
sudo dnf install gcc tar gzip

# Arch Linux
sudo pacman -Syu --needed gcc
```

Release checks run these commands in clean AlmaLinux 8.10 (glibc 2.28),
Debian 12 and 13, Ubuntu 24.04, Fedora 44, and Arch Linux containers, then
compile and run programs from the extracted archive. AlmaLinux 8 is the
glibc 2.28 baseline; installing packages does not make an older system
compatible.

## Install and use

```sh
cd ~/Downloads
sha256sum -c plenty-VERSION-x86_64-unknown-linux-gnu.tar.gz.sha256
tar -xzf plenty-VERSION-x86_64-unknown-linux-gnu.tar.gz
export PATH="$HOME/Downloads/plenty-VERSION-x86_64-unknown-linux-gnu/bin:$PATH"
plenty --version
plenty --compile hello.plenty -o hello
./hello
```

Add the `export` line to your shell's startup file to keep it. The directory
can be moved or renamed, and paths may contain spaces; update `PATH` to match.
Plenty writes nothing inside it: the [compiled-function cache](../02-architecture/cache.md)
lives in `~/.cache/plenty`, and temporary files in `TMPDIR`.

`plenty --version` prints the compiler's version and target:
`plenty X.Y.Z (x86_64-unknown-linux-gnu)`, with `X.Y.Z` replaced by the installed
version (including any pre-release suffix).

## Upgrade and switch versions

Each release extracts to its own versioned directory, so several can be
installed side by side. The first `plenty` on `PATH` is the one that runs:
point `PATH` at another release's `bin/` to switch, and check with
`plenty --version` or `command -v plenty`. Uninstall by deleting the directory.

## Troubleshooting

| Symptom | Cause and fix |
|---|---|
| `` failed to invoke linker driver `cc` ``, mentioning gcc or clang | No C compiler on `PATH`. Install the prerequisites, or pass `--linker clang` or another `cc`-compatible driver |
| `` linker driver `cc` failed `` naming `Scrt1.o` or `crt1.o`, `-lc`, `-lgcc_s`, or `-lpthread` | The C library development files are missing; install the prerequisites (`libc6-dev` on Debian and Ubuntu) |
| `GLIBC_2.xx not found` when starting `plenty` | The system is older than glibc 2.28 |

`plenty --check FILE` needs no linker.

## Programs you build

Generated programs link against the C library of the machine that compiles
them, through its `cc`. A program built on a new distribution can need a newer
glibc than Plenty itself and fail on an older system. To deploy to older
systems, compile there, or in a container of the oldest supported
distribution, as the release itself is built.

Maintainers: see [releasing Plenty](../../../development/releasing.md).
