# Plenty @VERSION@ for @TARGET@

Plenty is a statically typed language with Python-shaped syntax, compiled
ahead of time to native code. This archive contains the compiler:

| Path | Contents |
|---|---|
| `bin/plenty` | Compiler and project tool |
| `bin/plentyc` | Single-file compiler, without project commands |
| `LICENSE-MIT`, `LICENSE-APACHE` | Plenty's license: MIT or Apache-2.0, at your option |
| `THIRD-PARTY-NOTICES.md`, `licenses/` | Licenses of software built into the compiler |

The standard library and runtime are built into `bin/plenty`. You do not need
Rust, Cargo, or the Plenty source code. `std.dataframe` uses a separate optional
download from the same release:
`plenty-dataframe-@VERSION@-@TARGET@.tar.gz` and its `.sha256` checksum.
Its README explains installation and how to ship the shared engine with your
applications. `std.linalg` similarly uses
`plenty-linalg-@VERSION@-@TARGET@.tar.gz` and its checksum. Only programs reaching
an engine's native operations need that engine; ordinary programs need neither.

## Requirements

- GNU/Linux on the archive's @TARGET@ architecture with **glibc 2.28 or later**, for example RHEL/AlmaLinux/Rocky 8,
  Debian 10, Ubuntu 18.10, Fedora 29, or newer. The kernel must meet Rust's target minimum: 3.2 for x86_64 or 4.1 for aarch64; old kernels are not tested.
- Baseline CPU instructions: x86-64/SSE2 for x86_64 archives, ARMv8-A for
  aarch64 archives. Host CPU extensions are not required.
- A C compiler driver, `cc`, with the C library development files. Plenty
  generates machine code itself and runs `cc` only to link programs.

musl-based distributions such as Alpine are not supported by this archive.

Install the linking prerequisites with your distribution's package manager:

```sh
# Debian, Ubuntu
sudo apt-get update && sudo apt-get install gcc libc6-dev

# Fedora, RHEL, AlmaLinux, Rocky Linux
sudo dnf install gcc tar gzip

# Arch Linux
sudo pacman -Syu --needed gcc
```

These commands were tested with this release on AlmaLinux 8.10 (glibc 2.28),
Debian 12 and 13, Ubuntu 24.04, Fedora 44, and Arch Linux. Installing packages
does not make a system older than glibc 2.28 compatible.

## Install

Extract the archive anywhere and put its `bin/` directory on `PATH`. No
installer, root access, or fixed location is needed; the directory may be
moved or renamed.

```sh
cd ~/Downloads
tar -xzf plenty-@VERSION@-@TARGET@.tar.gz
export PATH="$HOME/Downloads/plenty-@VERSION@-@TARGET@/bin:$PATH"
plenty --version
```

Add the `export` line to `~/.bashrc` or your shell's startup file to keep it.
To verify the download first, run `sha256sum -c plenty-@VERSION@-@TARGET@.tar.gz.sha256`
in the download directory.

## Compile and run a program

Save this as `hello.plenty`:

```text
def main() -> Result[(), IoError]:
    print("Hello, Plenty!")?
    Ok(())
```

Then compile it to an executable and run it, or do both in one step:

```sh
plenty --compile hello.plenty -o hello
./hello
plenty hello.plenty
```

`plenty new NAME` creates a project; `plenty run` inside it builds and runs it.
`plenty --help` lists every command.

## Troubleshooting

- `` failed to invoke linker driver `cc` ``: no C compiler is installed or on
  `PATH`. Install the prerequisites above, or name another driver with
  `--linker PATH` (for example `--linker clang`).
- `` linker driver `cc` failed `` mentioning `Scrt1.o` or `crt1.o`, `-lc`, `-lgcc_s`, or
  `-lpthread`: the C library development files are missing. Install the
  packages above; on Debian and Ubuntu this is `libc6-dev`.
- `GLIBC_2.xx not found` when starting `plenty`: the system is older than
  glibc 2.28 and cannot run this release.

## Programs you build

Programs are linked against the C library of the machine that compiles them.
A program built on a new distribution can need a newer glibc than Plenty
itself. To run programs on older systems, compile them on the oldest system
you support, for example in a container.

## Upgrade or switch versions

Each release extracts to its own versioned directory, so several can be kept
side by side. Change `PATH` to the `bin/` directory of the release you want,
and check with `plenty --version`. To uninstall, delete the directory. Plenty
keeps a compiled-function cache in `~/.cache/plenty`; `plenty cache clean`
empties it.
