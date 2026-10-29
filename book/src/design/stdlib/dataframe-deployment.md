# Ship a dataframe application

A compiled program that uses `std.dataframe` needs
`libplenty_dataframe_v1.so` at run time. Ship the **same engine you compiled
against**, from the matching Plenty release, beside your executable. End
users need the host C libraries, but no Plenty compiler, Rust, Cargo or `cc`.
Programs without reachable dataframe operations need no engine.

## Bundle the executable and engine

After [installing the engine](dataframe-engine.md), keep its extracted archive
intact and select its library explicitly:

```sh
ENGINE=/path/to/plenty-dataframe-VERSION-x86_64-unknown-linux-gnu
export PLENTY_DATAFRAME_ENGINE="$ENGINE/lib/libplenty_dataframe_v1.so"
plenty --compile analysis.plenty --dist my-app
./my-app/analysis
```

`my-app` must not exist. Plenty stages and publishes the executable, the exact
engine used for linking, and its packaged notices together. No reachable engine
calls means no engine files; reachable linear algebra calls also add the linalg
engine and its notices. See [application distributions](../toolchain/01-building/distributions.md)
for metadata checks, supported input layouts, local-build packaging and CLI
restrictions. A bare installed `.so` without notices is not enough for `--dist`.

The resulting directory can be archived, installed elsewhere and renamed:

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

Keep the engine notices with distributions of the library, alongside any
notices your own application needs. The engine archive's README contains the
same distribution command for users who have no source checkout.

## Test on the oldest supported system

The prebuilt engine targets x86-64 Linux, glibc 2.28 or later and
`libgcc_s.so.1`. It does not support musl/Alpine, macOS, Windows or ARM.
Your executable links against the build machine's C libraries and can require
a **newer** glibc. Compile and test the application on the oldest distribution
you support; a baseline-compatible engine alone does not make your executable
portable. No particular memory bound is implied by the shared-library format.

Test the bundle in a clean machine or container without your original Plenty
installation. Run it from a different working directory, then move the bundle
and run it again. Test a missing-library failure too. The release smoke tests
exercise app-only relocation and a native Parquet query on each supported
test distribution after removing the compiler and original engine location.

## How library lookup works

This applies to every [native engine](../toolchain/01-building/engines.md).
Distribution output embeds an ELF `RUNPATH` of only the executable's own
directory (`$ORIGIN`), so a missing bundled library cannot fall back to the
original engine directory. A sibling `lib/` is not searched automatically.

For manual deployment with ordinary `--compile analysis.plenty -o analysis`,
copy the exact selected library beside the executable and all the archive's
notices into `dataframe-notices/`, as in the layout above. Ordinary output still
searches `$ORIGIN` **then the build-time engine directory**; that fallback can
mask a missing bundled library on the development machine. Prefer `--dist` for
shipping applications.

`PLENTY_DATAFRAME_ENGINE` chooses an engine at compile time; setting it when
running an already-built application does not affect library lookup.
`LD_LIBRARY_PATH` and `LD_PRELOAD` can override RUNPATH, so remove stale
overrides when testing or switching releases. Keep the application directory
and library search paths trusted. System-wide library installation is also
possible through the operating system's loader configuration.

The `v1` filename denotes the C ABI generation, not a promise that every
release exports every operation. New operations can add symbols without
changing this name. Keep releases matched and do not substitute a differently
built Polars library. `engine.json` records the engine's origin; `--dist` checks release/target/ABI
metadata and the copied library checksum. Ordinary compilation does not validate
its version; matching metadata is not proof of same-source identity for local builds.

| Failure | What to check |
|---|---|
| `cannot open shared object file` naming `libplenty_dataframe_v1.so` | Put the matching library beside the executable |
| `undefined reference` or `symbol lookup error` naming `pldf_v1_` | Check for an old engine or a loader environment override |
| `GLIBC_2.xx not found` | Check both the application and engine's platform requirements; rebuild on the oldest supported system |

Directly emitted object files need an appropriate external link command and
loader configuration; the automatic RUNPATH described here applies to
executables linked by Plenty.
