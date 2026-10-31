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

Direct native dependency builds use the pinned `cc` crate through
`plenty-runtime/native/cc_build.rs` for target compiler and archive selection.
SQLite and Brotli use native C options; ring selects its upstream assembly for
the target OS and architecture, including pregenerated COFF objects on Windows
x86-64. The GNU archive-symbol parser and panic-symbol redirects are Linux-only.
The standalone build-tool and source-selection tests do not by themselves
establish a working native compiler port.

Standard library Plenty sources are embedded in the compiler as well. `std.math`
is parsed and checked as an ordinary module, with a stable toolchain identity;
it is never loaded from the user's source root. Its algorithms and error policy
remain Plenty code, while numeric methods supply IEEE/platform primitives.
See the [standard library](../../stdlib/index.md) and [the math contract](../../stdlib/math.md).

The application archive supplies C `main`, calling `plenty_main`. A separate
library archive omits both startup and that dependency; shared-library export
maps hide runtime symbols. Both archives are built with the compiler.

Native emission validates the native GNU/Linux triple (`x86_64` or `aarch64`),
Cranelift's ISA and pointer type, and the packaged runtime target. Current layouts
depend on 64-bit pointers, 128-bit slots, little-endian metadata, the native C ABI
and fixed descriptor offsets. Cranelift names both Linux C conventions SystemV;
on AArch64 this implements AAPCS64, including aligned `u128` register pairs.
Ordinary Plenty functions keep the private Tail convention, with native adapters
at foreign boundaries. Layout assertions and native tests protect these
boundaries; selecting a linker does not validate a new platform.

## Target identities

The compiler's private `src/target.rs` module owns the admitted target table:
triple, architecture, OS, object format, pointer width, byte order and CPU
baseline. Frontend target facts and binary compatibility checks consume that
description. Tool discovery and command execution remain in `toolchain`;
target facts do not own compiler state or runtime OS services.

Cargo's `HOST` runs the build script; Cargo's `TARGET` identifies the compiler
executable's host. Those may differ when cross-building the compiler. Native
Plenty compilation requires the compiler host, packaged runtime and requested
program target to match. That check preserves cross-building the compiler
without offering cross-compilation through an installed Plenty compiler.
The admitted set remains the two GNU/Linux targets described in
[platform support](support.md).

## Runtime OS boundaries

The runtime keeps its compiler-facing ABI independent of native handles.
`threads` stores the native thread identity in one opaque word and delegates
creation and joining to Unix pthread or Windows HANDLE implementations.
Cancellation tokens keep their wake resource behind the native module; Linux
uses eventfd. Other systems retain the existing unavailable-readiness-wake
result, while ordinary token waiting uses the shared condition variable.

The generated mutex fast path remains the same atomic state protocol. Its
contended wait and wake operations use Linux futexes, Windows address waits,
or public pthread primitives on macOS. The macOS implementation shares static
parking storage and broadcasts on wake, avoiding private Darwin ulock APIs
and per-mutex allocations. Unrelated contended mutexes can wake together;
performance under heavy contention still needs native measurement.

Signal register access is selected by OS and architecture. Darwin's machine
context is a pointer to a separate record; Linux's is inline. The POSIX trap
and stack-overflow handlers use native errno access and signal conventions.
macOS stack exhaustion can arrive as SIGBUS or SIGSEGV, and its alternate
stack reservation accounts for Darwin's larger minimum. Existing host handlers
retain their signals. Windows uses vectored exception handlers and a per-thread stack guarantee.
Only registered checked-trap addresses are redirected; unrelated exceptions
continue through the host chain. Library threads do not install these handlers.

These boundaries are implementation work toward new ports, not admission of
macOS or Windows as supported compiler targets. The native qualification
workflow tests synchronization separately so failures elsewhere in the runtime
do not prevent exercising these primitives. Native Windows exception tests, object
and linker support, actor recovery, OS services and full native execution
remain qualification requirements in the [portability plan](../../../backlog.md#b18--macos-and-windows-implementation-sequence).

## Entry symbols

A standard module's native code is reachable only through its own entry
symbols. Generated code calls each runtime operation `N` through its own
exported function `plenty_op_<N>(arguments, descriptor, site)`; no dispatcher
shared between modules takes an operation number. `--gc-sections` keeps only the
entries a program calls and the code they reach, so a program that never
calls `std.net`, `std.process`, `std.crypto`, `std.compress`, `std.tls` or
`std.db.sqlite` links none of their runtime code. A dispatcher shared by
several modules would keep every module it reaches (issue #501). `hello world`
calls two entries. `BUDGETS` in `tests/test_executable_size.rs` bounds the
size of `hello world` and an empty program, and `NATIVE_MODULES` there checks
this for every module with native code; a new module adds a row there (see
[size budgets](../01-building/linking.md#size-budgets)).

The argument block holds three 16-byte inputs at offsets 0, 16 and 32, the
result at 48, and further operands at 64 and 80. The compiler always
initializes the first three input words, using zero for omitted inputs.
It initializes offsets 64 and 80 only for actual fourth and fifth operands;
the result and unused scratch words may be uninitialized. Runtime handlers
must not read those spare words, even if an optimizer would discard the value.

The compiler and runtime share the scratch-operand count in
`plenty-runtime/src/collection_opcodes.rs`. Code generation checks its actual
native operand count against that contract, including hidden operands such as
a column field index. The runtime reads only the declared scratch inputs and
supplies zero for unused elements of its local operand array. Adding a native
operation with more than three operands requires updating this shared contract.
No extra caller stores initialize spare words. Each entry inlines its constant
operation number and handler, so the operand count and handler selection fold
away rather than adding a runtime arity lookup.

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
thread reserves an alternate signal stack in its own entry frame: 16 KiB on
x86-64 and 64 KiB on AArch64, which has larger saved register contexts. It removes
the stack when Plenty code returns, with no heap allocation or mapping. Startup
checks the runtime page size and kernel minimum signal-stack requirement before
installing the handler; an inadequate reserved stack leaves the default signal
behavior in place. ARM qualification currently covers 4 KiB pages.
Generated frames larger than a page probe each page. The handler recognizes a
data fault below the entry frame and no more than one page below the faulting
stack pointer. AArch64 loop probes leave SP unchanged, so the compiler also
registers their exact store addresses in the read-only trap table. At those
addresses only, the handler checks the negative probe offset in x16 against the
frame size in x17 and uses the resulting probe address as the stack frontier.
Other faults retain the default action. Foreign code skipping stack pages is
outside this detection. Stack sizes remain the system's limits, and core-file
policy follows the system.

## Rust panics

A Rust panic in the runtime is a runtime defect, not a Plenty error. An
executable reports it as `error: internal runtime panic at FILE:LINE:COLUMN:
MESSAGE` on standard error and aborts with `SIGABRT`, without unwinding or a
backtrace. A failed Rust allocation reports `error: memory allocation of N bytes
failed` and aborts, unless an actor handler contains it as a trap or it
happens inside a reserved arena call, which returns an error instead: see
[RSA allocation](../../stdlib/crypto-keys.md#allocation-and-key-material).
Plenty's own traps and errors are unaffected.

std's panic handler would link its panic hook and backtrace symbolizer into
every executable, even with abort-on-panic. The application archive's link
arguments therefore redirect std's `rust_begin_unwind` and
`__rust_alloc_error_handler` to the runtime's handlers with `-Wl,--defsym`, and
`--gc-sections` drops std's versions. Measured on 2026-10-26, hello world's
`.text` is 69 KB with the redirect and 271 KB without it. Libraries keep std's
handlers, since their host links them.

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
position share the block. In an executable the block is a single x86 `ud2` or
AArch64 `udf`, with its own Cranelift trap code, and the object holds a trap table mapping
each such address to the check's kind and site record. Its addresses are
32-bit offsets the linker resolves, so a PIE applies no relocations to it
and its pages stay unread until a check fails. `plenty_main`
registers the table, and startup installs a `SIGILL` handler. It uses the
alternate signal stack by default; if a custom `SIGILL` action was already
installed, its `SA_ONSTACK` and `SA_RESTART` choices and signal mask are retained.
Unrelated signals invoke that saved action without removing Plenty's
checked-trap handler. On a kernel-raised `SIGILL` at a listed check address the
handler resumes the thread in the check's helper with the site as its
argument, so the report, exit status and thread behaviour match a direct
call. x86 restores a call-compatible stack; AArch64 restores PC, aligned SP and
argument registers for a native C helper entry. Any other
`SIGILL` goes to the previous action. The table lists only
functions reachable from exported functions and destructors, so linker
garbage collection still removes the rest. A function has 247 trap codes;
later checks, and every check in a library or `--fuzz` build,
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
