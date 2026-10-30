# Plenty @VERSION@ linear algebra engine for @TARGET@

This optional archive supplies `std.linalg`'s faer engine. Use the compiler and
engine from the **same Plenty release**. Applications that do not reach native
linear algebra operations need no engine. Matrices themselves are Plenty values.

Requires GNU/Linux on @TARGET@, glibc 2.28 or later, and `libgcc_s.so.1`. The baseline is x86-64/SSE2 or ARMv8-A, matching the archive target.

## Install

Verify the matching archive checksums with `sha256sum -c`, extract the archives,
and copy `lib/libplenty_linalg_v1.so` beside the compiler executable. Alternatively,
set `PLENTY_LINALG_ENGINE` to its absolute path before compiling. The engine's
directory must not contain a colon. Compile as usual:

```sh
plenty --compile analysis.plenty -o analysis
```

## Ship an application

Use explicit distribution output with the extracted archive intact:

```sh
ENGINE=/path/to/plenty-linalg-@VERSION@-@TARGET@
export PLENTY_LINALG_ENGINE="$ENGINE/lib/libplenty_linalg_v1.so"
plenty --compile analysis.plenty --dist my-app
./my-app/analysis
```

`my-app` must not exist. Plenty stages and publishes the executable, only reached
engines, and each engine's `engine.json`, `LICENSE-MIT`, `LICENSE-APACHE`,
`THIRD-PARTY-NOTICES.md` and `licenses/` tree in `NAME-notices/`. It checks the
manifest version, target, ABI, library name, checksum and library SONAME. A bare
installed `.so` is insufficient: use the archive layout, or place
`linalg-notices/` beside the installed library. `--dist` excludes `-o`,
`--legacy`, `--link-arg` and other compilation modes.

Users need compatible host C libraries, not Plenty, Rust, Cargo, Python or a C
compiler. This is not a static or cross-platform build. Distribution RUNPATH
searches only `$ORIGIN` (the executable's own directory), with no development
fallback. Test relocation from another working directory, and remove
`LD_LIBRARY_PATH`/`LD_PRELOAD` overrides. `PLENTY_LINALG_ENGINE` affects only
compilation. Matching metadata is not proof of identical local-build sources.

For manual deployment, ordinary `--compile FILE -o OUT` still searches `$ORIGIN`
then the build-time engine directory; copy the exact selected library beside the
executable and all engine notices into `linalg-notices/`. That fallback can mask
a missing library on the build machine; prefer `--dist` when shipping.

Build and test your application on your oldest supported distribution; linking
on a newer machine can raise the application's glibc requirement. The engine
archive alone does not establish your executable's compatibility.

`engine.json` records source commit, release version, checksum, ABI generation,
and baseline. ABI generation 1 is not a guarantee that every release has every
operation. Match releases; missing `plla_v1_` symbols indicate a stale engine.

The engine uses a pinned faer 0.24.4 fork with sequential optimized GEMM kernels
and fallible caller-owned packing/scratch storage. It does not start a thread
pool. A fixed four-slot pool reuses packing buffers (roughly 16 MiB retained in
this baseline build); concurrent calls can temporarily need more. Native
allocation failures return `AllocError`; they are outside Plenty's allocation
counters. GPU backends are not enabled. No Git checkout is needed to use this
prebuilt engine.
