# Install and ship the linear algebra engine

Native operations in [`std.linalg`](linalg.md) require
`libplenty_linalg_v1.so`. The separate `plenty-linalg-VERSION-TARGET.tar.gz`
release archive includes the prebuilt library, checksum, `engine.json`, and
dependency notices. Use the engine and compiler from the same release.

Install its `lib/libplenty_linalg_v1.so` beside the compiler, or set
`PLENTY_LINALG_ENGINE` to its absolute path. An embedded compiler can set
`CompileOptions.linalg_engine`, which takes precedence. Type checking and
unreachable native calls need no engine. Pure matrix construction/access does
not link it either. Programs reaching a native operation link only this engine,
without pulling in the Polars engine or faer code into the runtime archive.

From a source checkout, build it once:

```sh
python3 -B scripts/build_native_engine.py linalg --out target/release
```

The isolated `plenty-linalg/` Cargo workspace has a pinned lockfile. The build
targets baseline x86-64; compatible SIMD is selected by the underlying kernels.
It runs sequentially and creates no thread pool. The large algorithm and SIMD
implementation justifies a native dependency instead of reimplementing it in
Plenty.

## Allocation contract

The facade owns matrices in Plenty lists. Engine calls borrow strided numeric
views for one call and retain no handles or references. Faer's low-level APIs
receive scratch allocated with `MemBuffer::try_new`; other scratch buffers use
`Vec::try_reserve_exact`. High-level infallible allocating faer owners are never
used. Native scratch lives outside Plenty's allocation accounting, but allocation
failure is reported as `AllocError.OutOfMemory`.

The [faer fork](linalg.md#why-plenty-uses-a-faer-fork) adds fallibly allocated,
caller-owned packing workspaces. The engine acquires one before entering a
kernel, then supplies it to all three optimized GEMM dispatch paths through a
scoped borrow. This avoids initializing the backend's owning thread-local cache.
Rayon is disabled. Native tests deny allocations, including on the first call,
and exercise blocked decompositions and CPUs without AVX.

A fixed four-slot pool reuses workspaces. Its mutex is held only while taking
or returning a buffer, never during numerical computation; concurrent calls
receive independent buffers. The pool itself allocates nothing. A cache miss
allocates fallibly, and a full pool drops returned excess buffers. The baseline
x86 build retains at most roughly 16 MiB of packing storage until process exit;
active concurrent calls may temporarily require additional buffers. Output and
decomposition-scratch allocation costs remain visible through `Result`.

Source builds fetch the fork once, with the exact Git revision pinned in
`plenty-linalg/Cargo.lock`. For offline builds, first run
`cargo fetch --locked --manifest-path plenty-linalg/Cargo.toml` while online,
then preserve both Cargo's registry and Git caches. Prebuilt engine users need
neither Git nor a source checkout.

Unexpected Rust panics are caught at the C boundary and reported as
`NativePanic`; this is a guard against internal bugs, not an OOM recovery
mechanism. Engine operations write fresh Plenty-owned output buffers which are
discarded on error, leaving caller inputs intact.

## Deployment

Ship the compiled application, the exact library used to compile it, and all
engine notices together:

```text
my-app/
  analysis
  libplenty_linalg_v1.so
  linalg-notices/
    engine.json
    LICENSE-MIT
    LICENSE-APACHE
    THIRD-PARTY-NOTICES.md
    licenses/rust/...
```

The loader searches `$ORIGIN` (the executable's directory), then the build-time
engine directory. A sibling `lib/` directory is not searched automatically.
`PLENTY_LINALG_ENGINE` affects compilation, not an already-built program.
Check relocation without the original installation and without stale
`LD_LIBRARY_PATH` or `LD_PRELOAD`. Users need the host C libraries, not Rust,
Cargo, Python, or Plenty. Missing `plla_v1_` symbols indicate an old engine.

Prebuilt releases target x86-64 GNU/Linux with glibc 2.28 and `libgcc_s.so.1`.
Compile and test your application on your oldest supported distribution: a
compatible engine cannot lower the application's own glibc requirement.
See the shared [native-engine contract](../toolchain/01-building/engines.md).
