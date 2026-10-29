# Plenty @VERSION@ dataframe engine for @TARGET@

This optional archive supplies the prebuilt Polars engine for `std.dataframe`.
Use it with the compiler from **the same Plenty release**. Ordinary Plenty
programs do not need this download. No Rust, Cargo, Python or source checkout
is needed to install it or run compiled applications.

## Contents and requirements

| Path | Contents |
|---|---|
| `lib/libplenty_dataframe_v1.so` | Shared engine, C ABI generation 1 |
| `engine.json` | Plenty version, target, source commit, library checksum and glibc baseline |
| `LICENSE-MIT`, `LICENSE-APACHE` | Plenty license texts |
| `THIRD-PARTY-NOTICES.md`, `licenses/` | Engine dependency and Rust standard-library notices |

Requires x86-64 GNU/Linux with glibc **2.28 or later** and `libgcc_s.so.1`.
The release uses baseline x86-64; AVX2 is not required. This archive does not
support musl/Alpine, Windows, macOS or ARM. Compiling an application still needs
the compiler archive's documented `cc` and C development files.

## Install for compilation

Download both archives and checksums from the same Plenty release. In their
download directory, replacing paths as appropriate:

```sh
sha256sum -c plenty-@VERSION@-@TARGET@.tar.gz.sha256
sha256sum -c plenty-dataframe-@VERSION@-@TARGET@.tar.gz.sha256
tar -xzf plenty-@VERSION@-@TARGET@.tar.gz
tar -xzf plenty-dataframe-@VERSION@-@TARGET@.tar.gz
cp plenty-dataframe-@VERSION@-@TARGET@/lib/libplenty_dataframe_v1.so \
   plenty-@VERSION@-@TARGET@/bin/
export PATH="$PWD/plenty-@VERSION@-@TARGET@/bin:$PATH"
plenty --compile analysis.plenty -o analysis
```

Alternatively, leave the archive intact and set `PLENTY_DATAFRAME_ENGINE` to
the absolute path of its `lib/libplenty_dataframe_v1.so`. The compiler checks
this variable before looking beside its executable. Keep the original engine
archive or its notices for redistribution. Paths can contain spaces, but the
engine directory cannot contain a colon (`:`).

The `v1` library name identifies the C ABI generation; it does **not** mean
that every release exports the same operations. Upgrade the compiler and
engine together. Packaging checks all of that release's facade symbols, and
`--dist` checks the manifest version, target, ABI, library name and checksum.
Ordinary compilation does not inspect `engine.json`.

## Ship an application

Distribute the compiled executable, the exact engine used to build it, and
the accompanying notices. A simple relocatable layout is:

```text
my-app/
  analysis
  libplenty_dataframe_v1.so
  dataframe-notices/
    engine.json
    LICENSE-MIT
    LICENSE-APACHE
    THIRD-PARTY-NOTICES.md
    licenses/rust/...
```

Prefer explicit distribution output, with the extracted engine directory named
in `ENGINE` (rather than the bare library copied beside the compiler):

```sh
ENGINE="$PWD/plenty-dataframe-@VERSION@-@TARGET@"
export PLENTY_DATAFRAME_ENGINE="$ENGINE/lib/libplenty_dataframe_v1.so"
plenty --compile analysis.plenty --dist my-app
./my-app/analysis
```

`my-app` must not exist. Plenty stages and publishes the executable, only the
engines its reachable operations need, and all their notices. It validates the
packaged metadata and copied library checksum, then links that staged library.
A bare installed library without notices is insufficient: use the extracted
archive, or place `dataframe-notices/` beside the installed library as shown
above. `--dist` excludes `-o`, `--legacy`, `--link-arg` and non-compile modes.

Distribution RUNPATH is only `$ORIGIN` (the executable's own directory), with
no development-machine fallback. Move the whole directory, not just the
executable. End users need compatible host C libraries, but **not Plenty, Rust,
Cargo or a C compiler**. This is not a static or cross-platform build. Compile
and test on the oldest system you support: an application linked on a newer
system may require a newer glibc than the engine.

For manual deployment, ordinary `--compile analysis.plenty -o analysis` still
works: copy its executable, exact library and all notices into the layout above.
Ordinary output searches `$ORIGIN` then the build-time engine directory, which
can hide a missing bundled library; prefer `--dist` when shipping.

Test relocation from another working directory and test a missing-library
failure. `PLENTY_DATAFRAME_ENGINE` selects a library at **compile time** only.
`LD_LIBRARY_PATH` and `LD_PRELOAD` still override runtime lookup; remove stale
overrides and keep library search paths trusted. Matching metadata does not
prove that a local engine and compiler were built from identical sources.

## Troubleshooting

- `std.dataframe requires the optional Polars engine`: install the matching
  engine beside the compiler or set `PLENTY_DATAFRAME_ENGINE` before compiling.
- `libplenty_dataframe_v1.so: cannot open shared object file`: the application's
  runtime library is missing; restore it beside the executable.
- `undefined reference` or `symbol lookup error` mentioning `pldf_v1_`: a stale
  or incompatible engine was selected. Use the same release and check loader
  overrides. Do not rename an arbitrary Polars library to this filename.
- `GLIBC_2.xx not found`: the host is too old for the engine or application;
  build the application on your oldest supported host and use a compatible engine.

The engine is shared across programs; it is not copied into each executable.
Its own allocation failures may abort the process. `POLARS_MAX_THREADS` sets
its thread-pool size before first use.
