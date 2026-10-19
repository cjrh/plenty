# Compiler, runtime build, and native ABI

> Part of the [cross-platform assessment](../cross-platform.md).
> Proposed changes below are not implemented support.

## Build the compiler and the runtime for the same machine

`build.rs` currently emits two embedded Rust static archives (application and
library runtimes), native test hooks, and a fuzz driver. Its Rust invocations
honor Cargo's `TARGET`; its manual C compiler/archive commands do not implement
Cargo's target-specific tool discovery. The vendored ring files omit other
platforms' pregenerated assembly, and the selected sources explicitly target
x86-64 ELF. Merely setting `CC` or `cargo build --target` is insufficient.

Introduce one checked native-toolchain description consumed by runtime and
native dependency builds. Distinguish Cargo **HOST** (build scripts run here),
Cargo **TARGET** (compiler executable and embedded runtime run here), and any
future generated-code target. Use native CI initially. For later cross-builds,
require matching C tools, assembler, archiver, Rust target libraries and sysroot;
reject inconsistent inputs before producing a package.

Prefer a separate, orchestrated runtime build using upstream Cargo build scripts
and a locked dependency graph, then embed verified artifacts into the compiler.
This avoids maintaining copies of ring/libc/getrandom platform logic. It must
preserve the current self-contained compiler distribution, both runtime modes,
offline builds, feature agreement and reproducibility. Do not invoke Cargo
recursively against the same target directory from `build.rs` and risk lock
contention. Compare this approach with a dedicated target-aware native build
helper before changing the build architecture. If direct rustc builds remain,
explicitly reproduce and test every dependency's target-specific build-script
outputs, features, native inputs and licenses.

Audit SQLite compilation and platform defines, compression CPU feature paths,
crypto entropy/provider dependencies, native test hooks and libFuzzer separately.
The runtime workspace's Cargo build and the embedded build must behave alike.
On Windows choose compatible CRT linkage and a documented SDK/toolchain;
allocations crossing library boundaries must still be freed by their owner.
Keep Rust panic-abort and Plenty's no-unwinding contract. Do not introduce an
implicit Rust installation requirement for release users.

## Code generation and ABI correctness

Keep the internal Plenty Tail calling convention distinct from the target C
ABI. Derive native signatures from the target and audit **all** boundaries:
runtime helpers, entrypoints, threads, callbacks, imports, exports, dynamic
library discovery, and compiler-generated adapters. Replacing one SystemV
assignment does not cover the others. Small integer extension, booleans, float
arguments, aggregate returns, hidden return pointers, stack arguments and
callee-saved registers need C/Rust/Plenty round trips on each target.

Retain Plenty's specified layouts where possible, but separately establish
foreign layouts. Windows' C `long` is not a Unix 64-bit `long`; OS structs,
socket handles and `pthread_t` cannot be inferred from pointer size. Check i128
and floating-point lowering/libcalls, alignment of aggregates and atomics,
thread entry storage, and library contract hashes against the selected ABI.
CPU baselines must not accidentally depend on the release builder's processor.

Validate direct and indirect tail calls with spilled arguments, deep recursion,
large stack frames, closures, generators and worker threads. Keep stack probing
and guard-page protection: disabling them to make a build pass is not a port.
`plenty-runtime/src/stack_overflow.rs` requires separate architecture/OS handling,
including Darwin fault context and Windows exception/stack guarantees. Library
runtimes must continue respecting the embedding process's handlers. A fallback
may preserve safe termination without identical diagnostic text, but must be
specified and tested rather than misclassifying arbitrary memory faults.

## Objects, libraries, and linker drivers

| Concern | Linux GNU | Darwin | Windows MSVC |
| --- | --- | --- | --- |
| Object / executable | ELF `.o`, conventional executable name | Mach-O `.o`, conventional executable name | COFF `.obj`, PE `.exe` |
| Native libraries | `.a`, `.so` | `.a`, `.dylib` | static `.lib`, `.dll` plus import library |
| Driver and SDK | GCC/Clang with libc development files | Apple Clang and selected macOS SDK | MSVC or qualified clang-cl/lld-link with Windows SDK |
| Retention/export policy | Existing GNU sections/version scripts | Mach-O section/segment limits, exported symbols and dead stripping | COFF sections, export tables/import libraries and retention rules |

Represent link arguments as argument arrays, including paths with spaces;
current runtime link strings are whitespace-split. Add driver-specific command
construction and discovery rather than making MSVC pretend to be `cc -o`.
Audit startup, system libraries, minimum OS flags, executable naming, object
archive determinism and diagnostics for missing toolchains. LibFuzzer and
sanitizers may initially be explicitly unavailable where not qualified.

`src/library_metadata.rs` explicitly requires x86-64 ELF and
`.plenty.interface.*` sections. Define target-specific metadata sections and
validate architecture, endianness, bounds and contract identity for Mach-O and
COFF, including archive members. Mach-O naming constraints make simply reusing
the full ELF section name unsuitable. Enable the required object reader features
deliberately. Test object-only output, interface extraction, C headers, static
exports and dynamic exports independently of ordinary executables.

Runtime loading currently uses `dlopen`, `dlsym`, `dlclose` and GNU
`RTLD_NODELETE`. Darwin needs its own flags/lifetime implementation; Windows
needs wide-path loader calls, safe DLL search rules, exports and handle ownership.
Preserve the existing invariant that callable code stays resident after lookup
leases close, along with allocator provenance and instance identity checks.
If residency cannot be provided safely, reject the facility until it can; do
not map the existing close operation to an unload that leaves dangling code.

## Compiler tools are part of the port

Audit `src/project.rs`, `src/cache.rs`, `src/lsp.rs` and test helpers for Unix
exit status, executable suffixes, chmod/symlinks, replacement/rename semantics,
open-file deletion, shell invocations and cache directories. Cache keys already
include target/ISA information; preserve it and verify runtime, ABI and feature
changes invalidate artifact reuse. Test concurrent writes on Windows, where
open-file sharing rules differ, and choose platform-native cache locations while
preserving explicit overrides.

The LSP manually parses `file://` URIs and emits paths through lossy strings.
Add correct drive-letter/UNC handling, escaping, case behavior and non-ASCII
round trips; keep OS-native paths until presentation. Test package roots with
spaces, Unicode and mixed separators, Git dependencies, command arguments,
timeouts and child cleanup. Compiler startup and `--check` are necessary but
insufficient acceptance tests for a working developer installation.
