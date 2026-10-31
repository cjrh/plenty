# Installing a release

Each Plenty release on the project's GitHub Releases page provides a compiler archive
for each Linux target, such as `plenty-VERSION-x86_64-unknown-linux-gnu.tar.gz`
or `plenty-VERSION-aarch64-unknown-linux-gnu.tar.gz`, and its `.sha256` checksum.
Download the archive matching your machine's architecture.
Extract it anywhere and put its `bin/` directory on `PATH`. No installer, root
access, fixed location, Rust, Cargo, or source checkout is needed. The README
in the archive repeats these instructions.

For `std.dataframe`, also download the optional
`plenty-dataframe-VERSION-TRIPLE.tar.gz` for the same target and its checksum
from the same release. Follow [engine installation](../../stdlib/dataframe-engine.md)
and [application deployment](../../stdlib/dataframe-deployment.md). Ordinary
programs do not need this additional asset.

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

The standard-library sources and ordinary runtime are built into the
executables, so the compiler works from any directory. Dataframe execution
uses the separately installed shared engine.

## Requirements

| Requirement | Detail |
|---|---|
| Platform | x86-64 or ARM64 (AArch64) Linux with glibc 2.28 or later: RHEL/AlmaLinux/Rocky 8, Debian 10, Ubuntu 18.10, Fedora 29, or newer |
| Kernel | The Rust standard library's minimum, 3.2 on x86-64 and 4.1 on ARM64; old kernels are not tested |
| CPU | Any x86-64 CPU (the original SSE2 instruction set) or any ARMv8-A CPU; the compiler and generated programs use only that baseline |
| Linking | A `cc` driver, linker, startup files, and C library development files |

Rust's
[AArch64 Linux target](https://doc.rust-lang.org/rustc/platform-support/aarch64-unknown-linux-gnu.html)
sets the 4.1 kernel minimum. ARM64 archives are built and tested on native ARM
runners; page sizes other than 4 KiB are not tested.

musl distributions such as Alpine are outside the native target set; see
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
Debian 12 and 13, Ubuntu 24.04, Fedora 44, and Arch Linux containers for
x86-64, and AlmaLinux 8.10 and Debian 13 for ARM64, then compile and run
programs from the extracted archive. Each archive's README lists the systems
it was tested on. AlmaLinux 8 is the
glibc 2.28 baseline; installing packages does not make an older system
compatible.

## Experimental macOS and Windows downloads

Releases also publish experimental compiler archives for macOS on Apple silicon
and for 64-bit Windows:

| Archive | Requirements |
|---|---|
| `plenty-VERSION-aarch64-apple-darwin.tar.gz` | macOS 11 or later on Apple silicon; the Xcode Command Line Tools (`xcode-select --install`) for linking |
| `plenty-VERSION-x86_64-pc-windows-msvc.zip` | Windows 10, Windows Server 2016, or later on x86-64; Visual Studio Build Tools with the "Desktop development with C++" workload and the Windows SDK, which also supply the MSVC runtime the compiler uses |

They contain only `bin/plenty`, `bin/plentyc` (`.exe` on Windows), the README,
licenses and notices. The optional dataframe and linear algebra engines are
Linux shared libraries and are not available for these systems. The release
workflow builds each archive natively, runs the compiler/runtime unit tests and
platform-boundary checks, then extracts it on a macOS 15 or Windows Server 2025
runner with Rust and Cargo removed from `PATH`, and compiles and runs a small
program. Projects, most standard-library services such as SQLite and TLS, and
the diagnostics for a missing linker are not yet tested there, and several OS
services are incomplete; see [platform support](../02-architecture/support.md).

The executables are not notarized or code-signed for distribution. The macOS
executables carry the linker's ad-hoc signature; a browser download is
quarantined, so run `xattr -dr com.apple.quarantine` on the extracted
directory, or download with `curl`, which does not quarantine files. Windows may show a SmartScreen warning. The
archive's README gives installation and troubleshooting steps for each system.

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
