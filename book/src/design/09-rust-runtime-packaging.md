# Rust runtime packaging

`plenty-runtime` is a separate dependency-free workspace crate implemented in Rust.
Its internal C ABI exports use scalar arguments and pointers to fixed-layout
storage. Rust owns buffers; the compiler emits immutable type metadata. Narrowly scoped unsafe operations handle
generated-code pointers, flexible allocations, reference counts, and callbacks.
Plenty's static borrow checker still establishes source-level access permissions.

The compiler's build script invokes the selected rustc directly for Cargo's target,
avoiding recursive Cargo invocation and build-lock contention. It produces an
optimized static archive using ThinLTO and abort-on-panic, captures rustc's native
link requirements, and embeds both in the compiler. Per-program compilation
writes the Cranelift object and archive to a temporary directory, then runs `cc`
only as the linker driver. No C runtime sources remain. Rust compilation occurs
when building the compiler, with no rustc or LLVM invocation on the Plenty-program
compilation path. This is still native host compilation, not cross-compilation.

`--emit-object` separates code generation from linking; `--emit-runtime` extracts
the exact packaged archive and native dependencies for an external build system.
The runtime owns the executable startup symbol `main` and calls `plenty_main`
from one application object. See [execution commands](07-execution-commands.md).

The compiler also packages a library runtime archive without `main` or a
`plenty_main` dependency. C library output embeds this variant; the shared linker
export map hides its internal symbols. Both variants are compiled when building
Plenty, with their own native link requirements, rather than per library build.

The runtime includes GNU/Linux library-loading primitives: fallible path
conversion, eager local loading, bounded allocation-free symbol lookup, and
lookup-lease cleanup. Successful mappings use `RTLD_NODELETE`, so closing a
lookup lease leaves resolved code and static state resident until process exit.
Loader-owned memory and library constructors are outside Plenty's allocator;
native loading is execution, not sandboxed metadata inspection. These primitives
are the internal foundation for typed loading. Their platform contract follows
the [Linux dynamic loader API](https://man7.org/linux/man-pages/man3/dlopen.3.html).

Native emission is gated to `x86_64-unknown-linux-gnu`, matching the tested runtime
and compiler target. The build records Cargo's runtime target; emission also
checks the Cranelift ISA triple and pointer type. The ABI currently relies on
64-bit pointers, 128-bit value slots, little-endian metadata, System V runtime and
generator callback signatures, and fixed header/descriptor offsets. Existing
runtime layout assertions and native tests cover those layouts. Other platforms
need their own ABI and runtime validation before this gate is expanded; a custom
linker alone cannot provide it. Checking Plenty source remains independent of
native emission unless a target is explicitly requested.

The public signatures and memory layouts are checked by native regression tests
and compile-time layout assertions. Standalone runtime tests also run under Miri
with exposed-provenance semantics for the ABI's packed pointer slots. The
`runtime-checks` compiler feature enables a counting Rust allocator for native
integration tests, covering buffers, raw object storage, and temporary allocations.
Allocation-free test regions count allocation attempts, including allocations
that have already been freed by the end of the region.
The relocated-compiler test rejects any runtime C/Rust compilation at link time.
See [plenty-runtime/README.md](../runtime.md) for the boundary invariants
and validation commands.

A diagnostic measurement after this migration, on the same x86_64 Linux machine
and debug 100-function/6,194-byte workload with five measured repetitions, gave
1.499 ms for checking and 51.009 ms for complete AOT compilation with the optimized
archive. A first archive built without ThinLTO measured 100.508 ms for AOT, so
runtime optimization is deliberately paid during the compiler build. These are
single-session measurements, not controlled speedup claims against older baselines.
