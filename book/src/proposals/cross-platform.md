# Native macOS, Windows, and AArch64 support

> **Status: Investigation complete (2026-10-19).** This is a proposed direction,
> not a support announcement. No platform port is implemented by this document.
> Current contracts remain in [runtime packaging](../design/toolchain/02-architecture/runtime.md),
> [linking](../design/toolchain/01-building/linking.md), and the
> [standard library reference](../design/stdlib/index.md).
> Future work remains **B18**, with packaging under **B17** and path semantics
> coordinated with **B12** in the [backlog](../backlog.md).

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
| `aarch64-unknown-linux-gnu` | Rejected today; retains Linux APIs but changes native ABI/layout and assembly | First port; qualify GNU/Linux separately from other ARM systems |
| `aarch64-apple-darwin` | Rejected today; backend has Apple AArch64 calling convention and Mach-O support | First macOS port after common target/build work |
| `x86_64-apple-darwin` | Rejected today; different OS despite familiar ISA | Companion macOS port with its own native tests and deployment baseline |
| `x86_64-pc-windows-msvc` | Rejected today; Windows ABI, runtime and linker changes required | First Windows port; prefer one MSVC/SDK ecosystem initially |
| `aarch64-pc-windows-msvc` | Rejected today; ARM64 COFF relocation coverage is an additional backend risk | Separate feasibility gate before committing to a release |

All proposed targets are 64-bit little-endian. ARM32, big-endian AArch64, Android,
iOS, Linux musl, Windows GNU/MinGW, ARM64EC, and universal macOS binaries are
outside this initial scope. AArch64 support is not a promise to support every
ARM board, OS or ABI. WSL runs the Linux build; it is not native Windows support.
Minimum OS/kernel versions and CPU baselines must be chosen and tested per
target; do not inherit Rust's support tiers as Plenty's own guarantees.

## What the audit established

The Plenty baseline inspected is `0ec8057`; this assessment is source-based,
without macOS/Windows/ARM execution. Important implementation evidence:

| Location | Consequence |
| --- | --- |
| `src/toolchain.rs::validate_target_pair` | Only x86-64 Linux GNU is allowed, and requested target must match the embedded runtime |
| `src/codegen.rs::optimized_isa` | Native ISA, fixed pointer ABI, PIC and inline stack probes; runtime, entry and adapter signatures frequently hard-code `CallConv::SystemV` |
| `build.rs` | Direct `rustc --target TARGET` builds runtime dependencies; native C commands independently use `CC`/`AR` and GNU arguments; ring source list contains x86-64 ELF assembly |
| `plenty-runtime/src/stack_overflow.rs` | Unconditional x86-64 Linux signal/ucontext layouts and register indices |
| `plenty-runtime/src/aggregates/{filesystem,process,readiness,net}.rs` | Linux-specific implementations, native constants/layouts, and unsupported fallbacks in several paths |
| `src/library_metadata.rs`, `src/library.rs` | ELF/x86-64 interface inspection and GNU shared-library/export conventions |
| `stdlib/std/{path,env,net,report}.plenty` | Public byte-path semantics and numeric Linux error assumptions also live in Plenty source |
| `scripts/release/`, `.github/workflows/release.yml` | Linux GNU toolchain, ELF audit, container smoke tests and fixed target archive names |

Some runtime modules already use Rust's portable filesystem, clocks, locks and
containers internally. That is useful leverage, not evidence that the complete
runtime builds or behaves correctly on another system. Some fallback branches
return unsupported errors; others still import Unix-only types. A successful
`cargo check` alone would miss generated-program linking and ABI failures.

The pinned Cranelift **0.131.1** sources were inspected locally through
`cargo agents crate-info`: `cranelift-codegen/src/isa/call_conv.rs`,
`src/isa/aarch64/abi.rs`, and `cranelift-object/src/backend.rs`. They contain
Tail and Apple AArch64 conventions, AArch64 inline stack probing, and ELF,
Mach-O and COFF object selection. However, several AArch64 GOT/page relocations
in the object backend handle only ELF/Mach-O and otherwise panic. Object-format
selection alone therefore does not establish Windows ARM64 viability. Probe
actual Plenty-generated code before deciding whether an upstream backend update
or targeted contribution is needed. The separate out-of-line AArch64 probe
routine is unimplemented; Plenty currently requests **inline** probes.

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

- [Compiler, runtime build, and native ABI](cross-platform/compiler.md).
- [Standard library and OS semantics](cross-platform/stdlib.md).
- [Qualification, delivery, and reference implementations](cross-platform/delivery.md).

The recommended order is common target/build boundaries, Linux AArch64, macOS,
then Windows. Release engineering can be designed in parallel, but publishing
must depend on the runtime and stdlib qualification gates. Windows path and
readiness design should be resolved early enough to avoid baking Unix-only
assumptions into those common boundaries. No implementation schedule or support
promise is created here; B18 remains the scheduling authority.
