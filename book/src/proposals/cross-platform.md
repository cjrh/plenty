# Native macOS, Windows, and AArch64 support

> **Status: Research and design update (2026-10-31).** Linux AArch64 is
> implemented in the ARM feature branch, with qualification limits recorded in
> [the development guide](../development/linux-aarch64.md). macOS and Windows
> remain proposed. This document does not announce their support.
> Priorities and implementation work packages live in **B18** of the
> [backlog](../backlog.md), with B17 delivery and B12 path-contract dependencies.

Related design records cover [FFI and dynamic libraries](ffi-and-dynamic-libraries.md),
[generated export contracts](ffi-export-contracts.md), and the independent
[GCC backend candidate](codegen-gcc.md). This assessment concerns complete
native toolchains, not only instruction selection.

Plenty can plausibly reach these platforms while keeping Cranelift, but this
is a compiler/runtime/stdlib port, not just a release matrix expansion. Start
with native builds: a compiler running on one OS/architecture generates programs
for that same OS/architecture. Cross-compiling the compiler with Cargo is a
separate build-system concern; compiling a Plenty program for a different target
would additionally need a target ISA, runtime, SDK and linker. Defer that user
feature until native ports are qualified.

## Proposed scope

| Native host and generated-program target | Present evidence | Proposed treatment |
| --- | --- | --- |
| `x86_64-unknown-linux-gnu` | Explicitly admitted, tested and released | Preserve existing baseline throughout |
| `aarch64-unknown-linux-gnu` | Implemented; extensive QEMU execution, native/release gates outstanding | Preserve alongside x86-64 Linux during refactoring |
| `aarch64-apple-darwin` | Rejected today; backend has Apple AArch64 calling convention and Mach-O support | First macOS port after common target/build work |
| `x86_64-apple-darwin` | Not admitted | Deferred; no automatic promise from the ARM macOS port |
| `x86_64-pc-windows-msvc` / `x86_64-pc-windows-gnu` | Neither admitted | Compare SDK/CRT/dependency feasibility before choosing one initial Windows ABI |
| `aarch64-pc-windows-msvc` | Rejected today; ARM64 COFF relocation coverage is an additional backend risk | Separate feasibility gate before committing to a release |

All proposed targets are 64-bit little-endian. ARM32, big-endian AArch64, Android,
iOS, Linux musl, ARM64EC, and universal macOS binaries are
outside this initial scope. AArch64 support is not a promise to support every
ARM board, OS or ABI. WSL runs the Linux build; it is not native Windows support.
Minimum OS/kernel versions and CPU baselines must be chosen and tested per
target; do not inherit Rust's support tiers as Plenty's own guarantees.

## Evidence baseline

This update inspects Plenty `869e1cb8087c126ec26a620149bd74d2290c4ca7` and
Cranelift **0.137.0**. The October 19 assessment described an older x86-only
checkout; its assumptions about hard-coded native calling conventions and
x86-only crypto assembly are superseded by the ARM implementation.

The current compiler has a two-target description, derives native C calling
conventions through Cranelift, and builds matching ARM runtime dependencies.
It still hard-codes Linux in frontend target facts, assumes ELF in metadata and
object handling, and uses Linux runtime services. This is a useful starting
point, not a finished OS abstraction.

The immediate experimental scope is **ARM64 macOS and x86-64 Windows**. Preserve
the Linux milestone in Git; perform independent work in child worktrees and
merge validated increments into the portability branch. No local Mac or Windows
machine is assumed: actual OS execution requires CI runners. Configuring a job
or cross-linking an artifact is not execution evidence.

## Recommended architecture and decision boundaries

Keep portable language semantics and checked Plenty library policy above a
small target-specific native boundary. Centralize target data: triple, pointer
and scalar layout, C ABI, object format, linker flavor, artifact suffixes,
runtime identity, supported facilities and minimum platform. Each source of
this information must agree; an allowlist expansion is the last enablement step,
after validation, not the first workaround.

Use separate runtime platform modules for Linux, Darwin and Windows operations.
Prefer Rust `std` where its allocation, progress and error behavior meet Plenty's
contracts; use reviewed OS bindings where they do not. Avoid copying raw Linux
constants into a generic module or translating every OS failure to a guessed
Linux errno. Unsupported facilities need an explicit, documented outcome;
security-sensitive operations must fail closed rather than silently weaken.

The detailed assessment is split into:

- [Interface boundaries and source mapping](cross-platform/boundaries.md).
- [Compiler, runtime build, and native ABI](cross-platform/compiler.md).
- [Standard library and OS semantics](cross-platform/stdlib.md).
- [Qualification, delivery, and reference implementations](cross-platform/delivery.md).
- [Zig toolchain experiments](cross-platform/experiments.md).

Study both OS ports before fixing shared interfaces, then establish a minimal
macOS path and the selected Windows path. Port OS services against explicit
contracts before claiming parity. Keep Cranelift as the production backend;
Zig is a candidate C/dependency compiler and linker driver. User-facing Plenty
cross-compilation remains a separate admission decision.
