# Plenty @VERSION@ for @TARGET@ (experimental)

Plenty is a statically typed language with Python-shaped syntax, compiled
ahead of time to native code. This archive contains the compiler for macOS on
Apple silicon:

| Path | Contents |
|---|---|
| `bin/plenty` | Compiler and project tool |
| `bin/plentyc` | Single-file compiler, without project commands |
| `LICENSE-MIT`, `LICENSE-APACHE` | Plenty's license: MIT or Apache-2.0, at your option |
| `THIRD-PARTY-NOTICES.md`, `licenses/` | Licenses of software built into the compiler |

**This is an experimental preview.** The Linux archives are the supported
release platforms. On macOS, release checks compile and run small programs
using the core language, collections, generators and printing. Parts of the
standard library and runtime are incomplete here, including actor recovery and
several OS services. The optional `std.dataframe` and `std.linalg` engines are
not available for macOS.

The standard library and runtime are built into `bin/plenty`. You do not need
Rust, Cargo, or the Plenty source code.

## Requirements

- macOS 11 (Big Sur) or later on Apple silicon (arm64). Release checks run on
  macOS 15; older versions are not tested.
- The Xcode Command Line Tools, which provide `cc` and the system linker.
  Install them with `xcode-select --install`. Plenty generates machine code
  itself and runs `cc` only to link programs.

## Install

The executables are not notarized; they carry only an ad hoc signature. A
browser download is quarantined, and macOS refuses to start quarantined,
unnotarized programs. Download with `curl`, which does not set the quarantine
attribute, or remove it after extracting:

```sh
cd ~/Downloads
tar -xzf plenty-@VERSION@-@TARGET@.tar.gz
xattr -dr com.apple.quarantine plenty-@VERSION@-@TARGET@
export PATH="$HOME/Downloads/plenty-@VERSION@-@TARGET@/bin:$PATH"
plenty --version
```

Extract the archive anywhere; the directory may be moved or renamed. Add the
`export` line to `~/.zshrc` to keep it. To verify the download first, run
`shasum -a 256 -c plenty-@VERSION@-@TARGET@.tar.gz.sha256` in the download
directory.

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

`plenty --help` lists every command.

## Troubleshooting

- `` failed to invoke linker driver `cc` `` or an `xcrun: error` message: the
  Command Line Tools are missing. Run `xcode-select --install`.
- "cannot be opened because the developer cannot be verified" or "is damaged":
  the files are quarantined. Run the `xattr` command above.

## Upgrade or switch versions

Each release extracts to its own versioned directory. Change `PATH` to the
`bin/` directory of the release you want, and check with `plenty --version`.
To uninstall, delete the directory. Plenty keeps a compiled-function cache in
`~/.cache/plenty`; `plenty cache clean` empties it.
