# Runtime packaging

`plenty-runtime` is a Rust workspace crate using the standard library. Its
vendored dependencies include the RustCrypto crates behind
[`std.crypto`](../../stdlib/crypto.md#supply), zlib-rs behind
[`std.compress`](../../stdlib/compress.md#supply), rustls and its ring provider
behind [`std.tls`](../../stdlib/tls.md#supply), SQLite's C amalgamation behind
[`std.db.sqlite`](../../stdlib/sqlite.md#supply), and siphasher for
[collection hashing](collections.md#hash-storage).
Its private C ABI accepts fixed-width scalars and pointers to typed
storage. Generated code supplies valid initialized fields, bounded immutable
metadata, and checked ownership transfers; Rust owns buffers and narrowly scoped
unsafe operations handle raw storage, packed pointers, counts, and callbacks.

The compiler build invokes the selected rustc directly for Cargo's target,
avoiding recursive Cargo invocation and build-lock contention. It builds optimized
ThinLTO, abort-on-panic static archives and embeds them with native link requirements.
Program compilation extracts the archive and links through a `cc` driver. Runtime
source and the Rust toolchain are not needed by an installed compiler.

Standard library Plenty sources are embedded in the compiler as well. `std.math`
is parsed and checked as an ordinary module, with a stable toolchain identity;
it is never loaded from the user's source root. Its algorithms and error policy
remain Plenty code, while numeric methods supply IEEE/platform primitives.
See the [standard library](../../stdlib/index.md) and [the math contract](../../stdlib/math.md).

The application archive supplies C `main`, calling `plenty_main`. A separate
library archive omits both startup and that dependency; shared-library export
maps hide runtime symbols. Both archives are built with the compiler.

Native emission validates `x86_64-unknown-linux-gnu`, Cranelift's ISA and pointer
type, and the packaged runtime target. Current layouts depend on 64-bit pointers,
128-bit slots, little-endian metadata, System V callbacks, and fixed descriptor
offsets. Layout assertions and native tests protect these boundaries; selecting
a linker does not validate a new platform.

Boxed callable owners reuse the heap box prefix: a destruction header, concrete
content descriptor at byte 16, shared-call adapter address at byte 24, and typed
content slot at byte 32. Collection helper opcode 128 consumes an environment,
allocates its box, and returns `Result`; failure drops the concrete content.
Erased descriptors use kind `J` for managed ownership and opaque formatting, while
the heap prefix retains the concrete box descriptor for relocation and cleanup.

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

## Trap sites

An operation that can [trap](../../language/08-errors/02-allocation-and-traps.md)
passes its runtime helper a pointer to its source position. Positions are
sixteen-byte records (line, column, the offset to a length-prefixed file
name, and an embedded-standard-library flag) in one read-only table per
function, deduplicated by position and provenance. A per-function table lets
the function's IR be built on any thread with its offsets known. The module loader supplies provenance;
a filename that resembles a standard module cannot acquire it. File names
are relative to the source root, which keeps objects
[independent of the source directory](../01-building/linking.md). Each table
is a local symbol with no relocations, so each Plenty library in a program keeps
its own.

Inline checks (integer overflow, division by zero, shift counts, `unwrap`) compute the pointer
in their cold trap block, so the passing path gains nothing. A check whose
condition the compiler computes from constants and finds false, such as the
divisor's zero test in `x // 10` or the count test in `x >> 13`, emits no branch
or trap call, in debug and release builds. A check that may fire, or always
fires, keeps both. Collection helpers
that check an index, key, range parameter, or separator take it as an argument:
one address computation per call. The collection dispatcher writes its result at
a fixed offset in the argument block, which frees its fourth argument for the
site. Iteration reads and operations that cannot trap pass null. The runtime
reads the record only when the trap fires and formats the report directly to
standard error.

For objects containing embedded std functions, calls crossing between std and
user code save and restore a thread-local pointer to the nearest user call site.
Indirect and synthetic calls, generator advancement, and cleanup also preserve
this context where callbacks can run. Ordinary std-to-std calls also restore
context because their callee may tail-call through a user callback. Direct
user-to-user calls need no context operations. Tail calls transfer the pointer without retaining
the discarded frame. A std trap prints the original site and a caller note;
ordinary user traps ignore this context. Site records are immutable, so the
context needs no allocation. Worker threads start without a caller, and executor
jobs clear the context between jobs. The pointer names a site, not a stack trace.

A null site prints the message alone. Stack overflow, the token front end, and
runtime contract failures such as a library object from another loaded instance
have no site.

## Validation

Native tests and compile-time layout assertions check public helper signatures
and private layouts. Miri exercises runtime storage with exposed-provenance
semantics for packed pointers; it cannot execute generated Cranelift code.
`runtime-checks` enables allocation counting and failure injection. Allocation-free
markers count successful allocations on the calling thread and keep a separate
baseline for each thread; failed attempts do not count, and failure injection is
also thread-local. Live-heap checkpoints count
bytes across the process, so workers must be idle or joined and any retained
worker storage counts toward the checkpoint's allowance. All workers and their
results must be cleaned up before the final allocation-balance check. A relocated
compiler test rejects per-program C/Rust runtime compilation.
The [runtime contributor guide](../../../runtime.md) lists validation commands.
