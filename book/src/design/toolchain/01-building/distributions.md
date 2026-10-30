# Application distributions

Use explicit distribution output when shipping an application:

```sh
plenty --compile analysis.plenty --dist dist/analysis
./dist/analysis/analysis
```

`plentyc` accepts the same command. The executable is named after the source
file's stem (`analysis.plenty` becomes `analysis`), not the destination directory.
Paths may contain spaces. `--release`, `--no-strip`, `--module-root` and
`--package-graph` work as for ordinary executable compilation. `--dist` requires
`--compile FILE` and rejects `-o`/`--output`, `--legacy`, extra `--link-arg`
arguments and other output modes. A custom `--linker` must still produce the
required ELF lookup policy.

The destination must not exist, even as an empty directory, file or dangling
symlink. Choose a new name for each build. Plenty stages the output beside the
destination and publishes the complete directory without replacing an existing
path, including one created concurrently. Compilation, linking, missing notices
or publication failures leave no partial destination; temporary staging is
removed on normal error exits. Parent directories may be created. Abrupt process
termination can leave a hidden `.plenty-dist-*` staging directory, not a published
bundle.

## Contents and engine selection

A program without reachable native-engine calls produces only its executable.
Otherwise Plenty copies **only the engines selected for that link**, using the
ordinary [engine discovery and reachability rules](engines.md#the-engine-contract).
Unused imports and unreachable helpers do not add engines. Each used engine adds:

```text
dist/analysis/
  analysis
  libplenty_NAME_v1.so
  NAME-notices/
    engine.json
    LICENSE-MIT
    LICENSE-APACHE
    THIRD-PARTY-NOTICES.md
    licenses/...
```

`NAME` is `dataframe` or `linalg`; a program may need both. Executable names that
collide with engine or notices names are rejected. Application assets, application
licenses and non-engine dynamic dependencies are not collected automatically.
Add your own required files before distributing the directory.

The selected engine needs its packaged notices. Supported input layouts are:

- An extracted engine archive: `lib/libplenty_NAME_v1.so`, with `engine.json`,
  license files and `licenses/` at the archive root. Set `PLENTY_NAME_ENGINE`
  to the library's absolute path.
- A library beside `NAME-notices/`, whose contents match the layout above.
  This also works when installing beside the compiler.

Plenty checks manifest format 1, compiler release version, native target, ABI
generation and library name; requires source-commit and minimum-glibc provenance;
and verifies the **copied library's SHA-256** and SONAME. It links that staged
copy, so the bundled bytes are the bytes selected for linking. Missing files,
empty license directories, symlinks within notices, malformed metadata and
mismatches fail explicitly. No notices are fabricated or silently omitted.

This is consistency checking, not signature verification or proof that a local
engine was built from the compiler's source revision. Use trusted, matching
release artifacts or build and package from the same checkout. Ordinary
`--compile FILE -o OUT` does not require or validate engine metadata.

## Package a local engine

For local development, use `just build` for the compiler and both engines, or
`just engine linalg` / `just engine dataframe` for one engine. These recipes
install the library and matching `NAME-notices/` in `target/release/`, ready for
`--dist`. They record the engine's actual glibc requirements without claiming
the portable release baseline; see the [local build workflow](../../../development/workflow.md#build-a-complete-local-installation).

A bare library installed by `build_native_engine.py` is enough for ordinary
compilation, but not distribution output. Add local notices to it with:

```sh
python3 -B scripts/release/package-engine.py linalg \
  --engine target/release/libplenty_linalg_v1.so --out-dir target/release --local-install
```

For a **portable archive**, package a baseline-compatible build from the same
checkout with the release packager (substitute
`dataframe` and its library for the Polars engine):

```sh
python3 -B scripts/release/package-engine.py linalg \
  --engine /path/to/libplenty_linalg_v1.so --out-dir target/engine-packages
tar -xzf target/engine-packages/plenty-linalg-VERSION-x86_64-unknown-linux-gnu.tar.gz \
  -C target/engine-packages
export PLENTY_LINALG_ENGINE="$PWD/target/engine-packages/plenty-linalg-VERSION-x86_64-unknown-linux-gnu/lib/libplenty_linalg_v1.so"
plenty --compile analysis.plenty --dist dist/analysis
```

The packager needs the Rust standard-library license texts and engine dependency
sources to generate notices, and audits facade exports, SONAME, native
dependencies and the glibc 2.28 baseline. A library built on a newer host may fail
that audit. Build in the pinned release environment instead; the existing
`bash scripts/release/local-check.sh --skip-smoke` builds and packages locally
with Docker, without publishing. See [local release checks](../../../development/releasing.md#checking-locally).
Do not relabel another release's metadata to bypass checks.

## Relocation and platform limits

Bundled executables use only `$ORIGIN` (the executable's own directory) for
engine `RUNPATH`, **never the original engine directory**. A sibling `lib/` is
not searched. Ordinary builds retain `$ORIGIN` followed by the build-time engine
directory. `PLENTY_NAME_ENGINE` affects compilation only; loader configuration,
`LD_LIBRARY_PATH` and `LD_PRELOAD` still apply at run time. Keep those paths trusted.

Move the whole bundle, run it from another working directory without loader
overrides, and test that removing an engine makes loading fail even on the
build machine. End users need no Plenty, Rust or C compiler, but still need
compatible host C libraries. This is **not static linking or OS-independent
packaging**: native emission uses the installed compiler's x86-64 or AArch64
GNU/Linux target. Building on a
newer system can raise the executable's glibc requirement. Build and test on the
oldest system you support; engine metadata alone cannot establish portability.
