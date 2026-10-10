# Runtime packaging

`plenty-runtime` is a dependency-free Rust workspace crate using the standard
library. Its private C ABI accepts fixed-width scalars and pointers to typed
storage. Generated code supplies valid initialized fields, bounded immutable
metadata, and checked ownership transfers; Rust owns buffers and narrowly scoped
unsafe operations handle raw storage, packed pointers, counts, and callbacks.

The compiler build invokes the selected rustc directly for Cargo's target,
avoiding recursive Cargo invocation and build-lock contention. It builds optimized
ThinLTO, abort-on-panic static archives and embeds them with native link requirements.
Program compilation extracts the archive and links through a `cc` driver. Runtime
source and the Rust toolchain are not needed by an installed compiler.

The application archive supplies C `main`, calling `plenty_main`. A separate
library archive omits both startup and that dependency; shared-library export
maps hide runtime symbols. Both archives are built with the compiler.

Native emission validates `x86_64-unknown-linux-gnu`, Cranelift's ISA and pointer
type, and the packaged runtime target. Current layouts depend on 64-bit pointers,
128-bit slots, little-endian metadata, System V callbacks, and fixed descriptor
offsets. Layout assertions and native tests protect these boundaries; selecting
a linker does not validate a new platform.

## Stack overflow

An executable exhausting its stack reports `error: stack overflow` on standard
error and aborts with `SIGABRT` (shell status 134). This applies to its main,
scoped-worker, and executor-worker threads. It does not unwind destructors or
context exits. Libraries leave signal handling to their host and register no
alternate signal stack on threads they create.

Executable startup installs a `SIGSEGV` handler unless one already exists. Each
thread registers a 16 KiB alternate signal stack reserved in its own entry frame
and removes it when Plenty code returns, with no heap allocation or mapping.
Generated frames larger than a page probe
each page. The handler recognizes a data fault below the entry frame and no more
than one page below the faulting stack pointer; other faults retain the default
action. Foreign code skipping stack pages is outside this detection. Stack sizes
remain the system's limits, and core-file policy follows the system.

## Validation

Native tests and compile-time layout assertions check public helper signatures
and private layouts. Miri exercises runtime storage with exposed-provenance
semantics for packed pointers; it cannot execute generated Cranelift code.
`runtime-checks` enables allocation counting and failure injection. A relocated
compiler test rejects per-program C/Rust runtime compilation.
The [runtime contributor guide](../../../runtime.md) lists validation commands.
