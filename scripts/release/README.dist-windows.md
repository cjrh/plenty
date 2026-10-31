# Plenty @VERSION@ for @TARGET@ (experimental)

Plenty is a statically typed language with Python-shaped syntax, compiled
ahead of time to native code. This archive contains the compiler for 64-bit
Windows on x86-64:

| Path | Contents |
|---|---|
| `bin\plenty.exe` | Compiler and project tool |
| `bin\plentyc.exe` | Single-file compiler, without project commands |
| `LICENSE-MIT`, `LICENSE-APACHE` | Plenty's license: MIT or Apache-2.0, at your option |
| `THIRD-PARTY-NOTICES.md`, `licenses\` | Licenses of software built into the compiler |

**This is an experimental preview.** The Linux archives are the supported
release platforms. On Windows, release checks compile and run small programs
using the core language, collections, generators and printing. Parts of the
standard library and runtime are incomplete here, including actor recovery and
several OS services. The optional `std.dataframe` and `std.linalg` engines are
not available for Windows.

The standard library and runtime are built into `plenty.exe`. You do not need
Rust, Cargo, or the Plenty source code.

## Requirements

- 64-bit Windows 10 or later, or Windows Server 2016 or later, on x86-64.
  Release checks run on Windows Server 2025; other versions are not tested.
- Visual Studio Build Tools with the "Desktop development with C++" workload,
  which installs the MSVC linker, the C runtime libraries and a Windows SDK.
  Plenty generates machine code itself and finds `link.exe` through the Visual
  Studio installation; a developer command prompt is not required. The
  executables also need the Visual C++ runtime, which the Build Tools install.

## Install

The executables are not signed, so SmartScreen may warn when they first run.
Extract the archive anywhere, for example with Explorer's "Extract All" or in
PowerShell:

```powershell
cd $HOME\Downloads
Expand-Archive plenty-@VERSION@-@TARGET@.zip -DestinationPath .
$env:Path = "$HOME\Downloads\plenty-@VERSION@-@TARGET@\bin;$env:Path"
plenty --version
```

The directory may be moved or renamed. To keep it on `Path`, add the `bin`
directory under Settings > System > About > Advanced system settings >
Environment Variables. To verify the download first, compare
`(Get-FileHash plenty-@VERSION@-@TARGET@.zip).Hash` with the first field of
`plenty-@VERSION@-@TARGET@.zip.sha256`, ignoring case.

## Compile and run a program

Save this as `hello.plenty`:

```text
def main() -> Result[(), IoError]:
    print("Hello, Plenty!")?
    Ok(())
```

Then compile it to an executable and run it, or do both in one step:

```powershell
plenty --compile hello.plenty -o hello.exe
.\hello.exe
plenty hello.plenty
```

`plenty --help` lists every command.

## Troubleshooting

- `` failed to invoke linker driver `link.exe` ``: Visual Studio Build Tools
  with the C++ workload are not installed. Install them, then open a new
  terminal.
- `LNK1104: cannot open file` naming a `.lib` file such as `kernel32.lib`: the
  Windows SDK or MSVC libraries are missing. Add them in the Visual Studio
  Installer.
- `VCRUNTIME140.dll was not found`: install the Visual C++ Redistributable
  (x64) or the Build Tools above.

## Upgrade or switch versions

Each release extracts to its own versioned directory. Change `Path` to the
`bin` directory of the release you want, and check with `plenty --version`. To
uninstall, delete the directory. Plenty's compiled-function cache is used only
when `PLENTY_CACHE_DIR`, `XDG_CACHE_HOME` or `HOME` is set; `plenty cache clean`
empties it.
