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

## Entry symbols

A standard module's native code is reachable only through its own entry
symbols. Generated code calls each runtime operation `N` through its own
exported function `plenty_op_<N>(arguments, descriptor, site)`; no dispatcher
shared between modules takes an operation number. `--gc-sections` keeps only the
entries a program calls and the code they reach, so a program that never
calls `std.net`, `std.process`, `std.crypto`, `std.compress`, `std.tls` or
`std.db.sqlite` links none of their runtime code. A dispatcher shared by
several modules would keep every module it reaches. `hello world` calls two
entries, and its stripped executable is 356 KB rather than 790 KB (issue #501).

The argument block holds three 16-byte inputs at offsets 0, 16 and 32, the
result at 48, and further operands at 64 and 80. Each entry inlines the
operation's handler, so it does not pay for a range test over unrelated
operations.

`plenty_control`, `plenty_executor` and `plenty_channel` keep an operation
number. Each serves one facility (cancellation tokens, executors and channels
respectively), so a program that does not use it does not reach it.
Narrow helpers such as `plenty_list_len` and `plenty_text_find` take their operands in registers;
see [narrow native helpers](collections.md#narrow-native-helpers).

Boxed callable owners reuse the heap box prefix: a destruction header, concrete
content descriptor at byte 16, shared-call adapter address at byte 24, and typed
content slot at byte 32. Runtime operation 128 consumes an environment,
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

## Rust panics

A Rust panic in the runtime is a runtime defect, not a Plenty error. An
executable reports it as `error: internal runtime panic at FILE:LINE:COLUMN:
MESSAGE` on standard error and aborts with `SIGABRT`, without unwinding or a
backtrace. A failed Rust allocation reports `error: memory allocation of N bytes
failed` and aborts. Plenty's own traps and errors are unaffected.

std's panic handler would link its panic hook and backtrace symbolizer into
every executable, even with abort-on-panic. The application archive's link
arguments therefore redirect std's `rust_begin_unwind` and
`__rust_alloc_error_handler` to the runtime's handlers with `-Wl,--defsym`, and
`--gc-sections` drops std's versions. This removes about 190 KB of code from
hello world. Libraries keep std's handlers, since their host links them.

## Trap sites

An operation that can [trap](../../language/08-errors/02-allocation-and-traps.md)
passes its runtime helper a pointer to its source position. Positions are
sixteen-byte records (line, column, the offset to a length-prefixed file
name, and an embedded-standard-library flag) in one read-only table per
object. Each function numbers its own records, deduplicated by position and
provenance, so its IR is built on any thread with the offsets known; the
thread placing the function appends them to the object's table and carries
the function's base offset in its relocation, and file names follow all the
records, each named once. The module loader supplies provenance;
a filename that resembles a standard module cannot acquire it. File names
are relative to the source root, which keeps objects
[independent of the source directory](../01-building/linking.md). The table
is a local symbol with no relocations, so each Plenty library in a program keeps
its own.

Inline checks (integer overflow, division by zero, shift counts, float-to-integer
casts, `unwrap`) branch to a cold trap block; checks of one kind at one
position share the block. In an executable the block is a single `ud2`
with its own Cranelift trap code, and the object holds a trap table mapping
each such address to the check's kind and site record. Its addresses are
32-bit offsets the linker resolves, so a PIE applies no relocations to it
and its pages stay unread until a check fails. `plenty_main`
registers the table, and startup installs a `SIGILL` handler on the
alternate signal stack. On a kernel-raised `SIGILL` at a listed address the
handler resumes the thread in the check's helper with the site as its
argument, so the report, exit status and thread behaviour match a direct
call; any other `SIGILL` goes to the previous action. The table lists only
functions reachable from exported functions and destructors, so linker
garbage collection still removes the rest. A function has 247 trap codes;
later checks, and every check in a library, `--fuzz` or `--coverage` build,
compute the pointer in the cold block and call the helper. Either way the
passing path gains nothing. A check whose
condition the compiler computes from constants and finds false, such as the
divisor's zero test in `x // 10` or the count test in `x >> 13`, emits no branch
or trap call, in debug and release builds. A check that may fire, or always
fires, keeps both. Collection helpers
that check an index, key, range parameter, or separator take it as an argument:
one address computation per call. An operation entry writes its result at a
fixed offset in the argument block, which frees an argument for the site. Iteration reads and operations that cannot trap pass null. The runtime
reads the record only when the trap fires and formats the report directly to
standard error.

For objects containing embedded std functions, user calls into std save and
restore a thread-local pointer to the nearest user call site. User code also
saves and restores it around generator advancement, cleanup, and collection
operations that can run callbacks. Std code saves and restores it only around
indirect calls and direct calls to functions that may change it: a function
may change the context when it tail-calls std from user code, makes an indirect
tail call, calls such a function in tail position, or is a generator or a
function without Plenty source. The runtime restores the context itself after
each destructor, generator resumption, and sort-key comparison it runs, so no
runtime helper changes it. Direct
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
