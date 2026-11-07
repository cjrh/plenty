# Install the dataframe engine

The ordinary compiler build does not compile Polars. Importing
`std.dataframe` can be type-checked without its engine. Linking needs the
engine only when retained native code reaches a dataframe operation,
including calls through dependencies. An unused import or unused local
helper does not add a Polars dynamic dependency.

## Use the release engine

Linux [release archives](../toolchain/01-building/installing.md) include
`bin/libplenty_dataframe_v1.so` and its `bin/dataframe-notices/`. The compiler
finds the library beside itself, so no separate installation is needed. Set
`PLENTY_DATAFRAME_ENGINE` only to select a different library. The notices
directory holds `engine.json`, license files and dependency notices for
redistribution. The manifest records the Plenty version, source commit, target, ABI generation,
library checksum and minimum glibc. It is descriptive metadata, not a loader
configuration or a compiler-enforced version check.

The prebuilt engine supports x86-64 and ARM64 GNU/Linux with glibc **2.28 or later**
and `libgcc_s.so.1`, with no AVX2 requirement. Rust, Cargo and a source checkout
are unnecessary. The experimental Windows x86-64 release also includes
`bin/plenty_dataframe_v1.dll` and `bin/dataframe-notices/`, requiring Windows 10
or Windows Server 2016 or later and the MSVC runtime. Its manifest records
`minimum_windows` instead of `minimum_glibc`. The experimental macOS ARM64
release includes `bin/libplenty_dataframe_v1.dylib` and the same notices layout,
requiring macOS 11 or later. Its manifest records `minimum_macos`.
musl/Alpine and other native targets remain unsupported.
The compiler still needs its [linking prerequisites](../toolchain/01-building/installing.md).
See [shipping dataframe applications](dataframe-deployment.md) for runtime
library placement, notices and compatibility checks.

## Build from source

For the library plus local `--dist` metadata/licenses, use `just engine dataframe`.
`just build` builds the compiler and both engines; see the
[local build recipes](../../development/workflow.md#build-a-complete-local-installation).
The lower-level command below installs only the library.

From a Plenty source checkout, with a stable Rust toolchain:

```sh
python3 scripts/build_dataframe_engine.py --out target/release
```

Build the compiler separately with `cargo build --release --locked`.
The script pins Polars 0.55.2 with `lazy`, `parquet`, `csv`, `strings`, `regex`,
`temporal`, `dtype-date`, `dtype-datetime`, `semi_anti_join` and `asof_join`,
uses three Cargo jobs by default, and installs
`libplenty_dataframe_v1.so`. Use
`--target-dir PATH` to reuse engine build artifacts and `--jobs N` to
control build concurrency. It forces baseline `x86-64` code generation
and the system C linker, overriding ambient CPU flags; AVX2 is not required.
The script supports native x86-64/ARM64 GNU/Linux builds, Linux cross targets,
and native `x86_64-pc-windows-msvc` and `aarch64-apple-darwin` builds. On Windows it uses MSVC, installs
`plenty_dataframe_v1.dll`, and avoids ELF linker flags. On macOS it builds for
Apple M1 with `MACOSX_DEPLOYMENT_TARGET=11.0`, installs
`libplenty_dataframe_v1.dylib` with identity
`@rpath/libplenty_dataframe_v1.dylib`, and uses the native C linker.
Build on a system with a compatible glibc version for the deployment host.
This source-build command does not impose the release's glibc 2.28 baseline;
the release workflow builds and audits the engine inside its pinned container.

The extra string features support the native expression API. Polars 0.55.2's
string dispatch imports its optional time module even without string-to-date
operations, so this feature combination also needs `temporal` to compile.
The date/datetime dtype features support [temporal columns and expressions](dataframe-datetime.md).
Checked parsing and calendar validation also use the pinned Chrono dependency.
Ordinary compiler/runtime builds still do not build Polars.
The `parquet` feature already enables Polars' streaming engine; CSV support
adds only `csv`, not a second execution engine. The `semi_anti_join` feature
supports native membership joins; `asof_join` supports checked integer-key
as-of matching. Temporal and grouped as-of APIs are not exposed.

## Discovery and compatibility

The dataframe engine follows the general [native engine contract](../toolchain/01-building/engines.md).
The compiler looks beside its own executable, or at the file named by
`PLENTY_DATAFRAME_ENGINE`. An explicit environment path takes precedence.
The Rust compilation API also offers `CompileOptions.dataframe_engine`.
Missing engines produce an installation hint at link time.

```sh
export PLENTY_DATAFRAME_ENGINE=/opt/plenty/libplenty_dataframe_v1.so
plenty --compile analysis.plenty -o analysis
```

The resulting executable contains a versioned shared-library dependency,
not a copy of the entire engine. For shipping, prefer
[distribution output](dataframe-deployment.md): `--compile analysis.plenty --dist my-app`
copies the selected engine and its packaged notices, with only `$ORIGIN` lookup.
The ordinary `-o` output above has RUNPATH searching its own directory
and the engine's build-time directory. Deploy the executable beside the
matching `libplenty_dataframe_v1.so`, or install that library in the
loader's search path. Keep compiler bindings and engine from the same
Plenty release; the `v1` name denotes the C ABI generation, not compatibility
with arbitrary future Polars builds. Directly emitted objects also require
this engine when externally linked. Engine directories containing a colon
are rejected because ELF RUNPATH uses that character as a separator.

Rebuild/reinstall the engine when updating the compiler's dataframe API.
New query operations add version-1 symbols; an older engine cannot satisfy
their native links even though its filename is unchanged.
The system loader's `LD_LIBRARY_PATH` and `LD_PRELOAD` can override RUNPATH;
keep stale engine copies out of those paths when switching installations.

Normal programs need neither this library nor Rust. They do not gain a
`DT_NEEDED` entry for Polars. A project fingerprint includes the selected
engine path and metadata, so replacing or relocating it triggers relinking.
Already-linked executables still need their dynamic dependencies at run time.

## macOS application linking

The macOS compiler uses the same option/environment/beside-compiler discovery
precedence and links the selected ARM64 dylib directly. Ordinary compilation
copies it beside the executable, as described in
[engine copies beside applications](#engine-copies-beside-applications), which
preserves old file mappings and cached signatures. Invalid selected engines report the build
script needed to produce a compatible dylib. `--dist` adds the validated manifest, licenses
and notices, and checks the copied dylib identity and checksum. The application
uses `@loader_path` to resolve the library's `@rpath` identity from its own
directory, without a fallback to the original build directory.

Release packaging checks ARM64 Mach-O headers, a minimum macOS version no newer
than 11, system-only dependencies, the versioned dylib identity, facade exports
and the code signature. Apple tools verify the linker-generated ad-hoc
signature; the release remains unnotarized. Keep the dylib unmodified and
beside the executable when moving the application. Native CI tests Parquet
queries and application-only relocation.

## Windows application linking

The Windows compiler selects the DLL through the same option/environment/beside-
compiler precedence. It derives an MSVC import library from that DLL's exported
C symbols using `lib.exe`, supplied by Visual Studio Build Tools. Ordinary
compilation copies the DLL beside the output executable; `--dist` includes the
DLL and validated notices. Programs use Windows DLL lookup, with the application
directory searched by the loader. Keep the DLL beside the executable, and use
trusted application directories and `PATH` entries. Type-checking needs no DLL.

## Engine copies beside applications

On Windows and macOS, ordinary compilation links the selected engine and, after
the link succeeds, places a copy beside the executable. A failed link leaves an
existing application and its engine unchanged. When the existing copy already
has the same contents, it is not rewritten. Otherwise the new engine is copied
to a fresh file in the output directory and renamed over the old one, so
programs that have the old engine open keep it. Windows refuses to replace a DLL
that a running program has loaded; the error names the file, and closing the
program and compiling again replaces it. `plenty FILE` runs the program from a
temporary directory and hard-links the engine there when the file system
allows, instead of copying it.

## Development checks

To exercise the optional component after building it:

```sh
export PLENTY_DATAFRAME_ENGINE="$PWD/target/release/libplenty_dataframe_v1.so"
export PLENTY_DATAFRAME_TESTS=1
cargo test --locked --test integration test_std_dataframe::
cargo test --locked --test integration test_tutorial::dataframe_examples_with_optional_engine
```

Regular tutorial checks type-check dataframe examples without the engine.
The separate dataframe CI job builds it and executes those examples and
the real-engine integration tests. These tests cover bulk transfer, query
plans, nulls, errors, Parquet and worker threads, as well as absence of the dynamic dependency
from programs that do not use it. The engine crate's own tests run with
`python3 -B scripts/build_native_engine.py dataframe --out target/release --validate`.
[How std.dataframe wraps its engine](../facilities/04-interop/dataframe-binding.md)
describes the thread contract each engine type and call must keep.
