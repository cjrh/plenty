# Plenty @VERSION@ linear algebra engine for @TARGET@

This optional archive supplies `std.linalg`'s faer engine. Use the compiler and
engine from the **same Plenty release**. Applications that do not reach native
linear algebra operations need no engine. Matrices themselves are Plenty values.

Requires x86-64 GNU/Linux, glibc 2.28 or later, and `libgcc_s.so.1`. AVX2 is not
required. Other platforms are not supported by this release.

## Install

Verify the matching archive checksums with `sha256sum -c`, extract the archives,
and copy `lib/libplenty_linalg_v1.so` beside the compiler executable. Alternatively,
set `PLENTY_LINALG_ENGINE` to its absolute path before compiling. The engine's
directory must not contain a colon. Compile as usual:

```sh
plenty --compile analysis.plenty -o analysis
```

## Ship an application

Keep the exact engine used for compilation, notices, and application together:

```sh
ENGINE=/path/to/plenty-linalg-@VERSION@-@TARGET@
mkdir -p my-app/linalg-notices
cp analysis "$ENGINE/lib/libplenty_linalg_v1.so" my-app/
cp "$ENGINE/engine.json" "$ENGINE/LICENSE-MIT" "$ENGINE/LICENSE-APACHE" \
   "$ENGINE/THIRD-PARTY-NOTICES.md" my-app/linalg-notices/
cp -R "$ENGINE/licenses" my-app/linalg-notices/
./my-app/analysis
```

Users need no Plenty, Rust, Cargo, Python or C compiler to run the bundle.
The executable's RUNPATH searches its own directory first, then the build-time
engine directory. Test relocation without the original installation, and remove
`LD_LIBRARY_PATH`/`LD_PRELOAD` overrides. Those overrides can mask a missing or
stale bundled engine. `PLENTY_LINALG_ENGINE` only affects compilation.

Build and test your application on your oldest supported distribution; linking
on a newer machine can raise the application's glibc requirement. The engine
archive alone does not establish your executable's compatibility.

`engine.json` records source commit, release version, checksum, ABI generation,
and baseline. ABI generation 1 is not a guarantee that every release has every
operation. Match releases; missing `plla_v1_` symbols indicate a stale engine.

The engine uses patched faer 0.24.4's sequential, allocation-free SIMD matrix
kernels and fallible scratch allocation. It does not start a thread pool.
Native allocation failures return `AllocError`; they are outside Plenty's
allocation counters. Upstream heap-packed GEMM and GPU backends are not enabled.
