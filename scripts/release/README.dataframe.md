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
engine together. Packaging checks all of that release's facade symbols, but
the compiler does not inspect `engine.json` or enforce an exact version match.

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

For example, with the extracted engine directory named in `ENGINE`:

```sh
ENGINE="$PWD/plenty-dataframe-@VERSION@-@TARGET@"
mkdir -p my-app/dataframe-notices
cp analysis "$ENGINE/lib/libplenty_dataframe_v1.so" my-app/
cp "$ENGINE/engine.json" "$ENGINE/LICENSE-MIT" "$ENGINE/LICENSE-APACHE" \
   "$ENGINE/THIRD-PARTY-NOTICES.md" my-app/dataframe-notices/
cp -R "$ENGINE/licenses" my-app/dataframe-notices/
./my-app/analysis
```

The executable's RUNPATH searches its own directory (`$ORIGIN`) first, then
the engine's build-time directory. Moving the whole `my-app` directory works;
moving only the executable does not bring its library with it. End users need
the host C libraries, but **not Plenty, Rust, Cargo, or a C compiler** to run
the application. An application linked on a newer distribution can require a
newer glibc than the engine: build and test on the oldest system you support.

Test the bundle in a clean environment without the original compiler/engine
installation. Otherwise the build-time RUNPATH can hide a missing bundled
library. `PLENTY_DATAFRAME_ENGINE` selects a library at **compile time**; it
does not configure an already-built application's loader. `LD_LIBRARY_PATH`
and `LD_PRELOAD` can override RUNPATH, so remove stale overrides. Do not put
untrusted libraries beside your executable or in these paths.

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
