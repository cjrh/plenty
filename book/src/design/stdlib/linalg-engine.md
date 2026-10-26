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

Upstream faer's packed GEMM paths allocate infallibly, including inside some
decompositions. The bundled source has narrowly documented changes to route
native multiplication through allocation-free nano-gemm SIMD kernels and the
generic triangular/indexed kernels. Rayon is disabled. This trades some large
matrix throughput for recoverable failure; it is not a claim to reproduce
upstream faer's best benchmark results. The native tests deny each allocation
in turn and exercise sizes above the original packed-GEMM threshold.

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
