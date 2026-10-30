# Linux AArch64 implementation plan

Research baseline: `251b5277c97fc15a5e87b373474b3336d8142e12`, 2026-10-30.
This document records the implementation plan; it is not a release support
announcement. The source tree now implements the ARM path, with the reproducible
harness and measured limits documented in
[Linux AArch64 development](book/src/development/linux-aarch64.md). The isolated
backend/C ABI probe and runtime suites have passed under QEMU user mode;
full compiler workflow qualification is continuing under an ARM Linux guest.
Native hardware, the ARM glibc 2.28 deployment floor, other page sizes and
optional-engine execution remain separate admission gates.
It expands B18 in the [backlog](book/src/backlog.md), with release work under
B17 and actor portability under B85. Keep priorities there and implementation
status in local issues/PRs. This worktree-local plan was requested explicitly.

## Recommendation and scope

Start with **native `aarch64-unknown-linux-gnu`**: an ARM64 Linux compiler that
builds ARM64 Linux programs. Retain x86-64 Linux throughout. Linux is a good
first step because ELF, GNU linking, byte paths, pthreads, epoll, inotify and
dynamic loading remain available. CPU instructions, C ABI details, signal
contexts, syscall numbers and even some Linux flags still change.

Cranelift already supports AArch64; no new machine-code backend is proposed.
Its [project documentation](https://cranelift.dev/) explicitly lists AArch64.
The substantial work is adapting Plenty's runtime/build assumptions and proving
the complete compiler/runtime combination works.

Recommended initial contract:

- 64-bit little-endian GNU/Linux; retain Plenty's existing language layouts.
- Generic ARMv8-A CPU baseline. Do not require optional LSE, SVE or crypto
  extensions merely because the builder has them. Qualify runtime dispatch.
- Aim for the existing glibc 2.28 release floor, subject to actually building
  and running in an ARM64 baseline environment. Record a tested kernel floor.
- Preserve language, standard-library, static/shared-library and actor behavior.
  Optional dataframe/linalg engines have a separate qualification gate.
- No user-facing cross-compilation, musl, Android, ARM32, big-endian ARM,
  macOS or Windows in this increment. Cross-building the Rust compiler for
  testing is a build facility, distinct from Plenty cross-compilation.
- Keep scalar `std.simd` behavior first; ARM vector acceleration can follow.

The [earlier assessment](book/src/proposals/cross-platform.md), merged as local
PR #355, remains useful rationale. It inspected Cranelift 0.131.1 and an older
Plenty revision. This plan checks the current 0.137.0 dependencies and newer
trap, actor, assembly-dump and optional-engine code.

## Findings at the research baseline

| Area | Evidence and implication |
| --- | --- |
| Admission and ISA | [toolchain.rs](src/toolchain.rs) only admits x86-64 GNU/Linux. [codegen.rs](src/codegen.rs) constructs a native baseline ISA and checks it against the packaged runtime. Keep the equality check; a new allowlist entry alone is unsafe. |
| Target facts | [frontend/target.rs](src/frontend/target.rs) already recognizes `aarch64` and 64-bit pointers. Target conditionals need native ARM tests, not a new language feature. |
| Dependency build | [build.rs](build.rs) runs `rustc --target TARGET` directly, but selects x86 ELF ring assembly and fixed BLAKE3 x86 cfgs; C tools use independent `CC`/`AR`. Audit dependency build-script requirements, including transitive ARM-only dependencies. |
| C boundary | [codegen.rs](src/codegen.rs) and adapters frequently specify `CallConv::SystemV`. In Cranelift this also describes the Linux AArch64 C convention: the name is not itself an x86-only bug. Still audit actual argument/return lowering, especially `u128`, against Rust/C. |
| Runtime traps | [traps.rs](plenty-runtime/src/traps.rs) reads x86 RIP/RSP/RDI/RSI and reconstructs x86 call state. [codegen/traps.rs](src/codegen/traps.rs) describes two-byte `ud2` checks. ARM context restoration and instruction size must be handled deliberately. |
| Stack/signal ABI | [stack_overflow.rs](plenty-runtime/src/stack_overflow.rs) hard-codes x86 signal layouts and page-fault bits. [signals.rs](plenty-runtime/src/signals.rs) is x86-only. Preserve handlers, thread guards and library-host coexistence. |
| Linux constants | [mutex.rs](plenty-runtime/src/mutex.rs) uses futex syscall 202; vendored libc defines ARM64's as 98. [filesystem.rs](plenty-runtime/src/aggregates/filesystem.rs) uses x86 `O_DIRECTORY`/`O_NOFOLLOW` values despite a Linux/64-bit gate; ARM64 values differ. These are correctness/security boundaries. |
| Existing portable pieces | Filesystem metadata uses `statx`, not an x86 `struct stat`. [readiness.rs](plenty-runtime/src/aggregates/readiness.rs) already distinguishes packed x86 epoll records from other architectures. Verify and reuse these decisions. |
| Actors | [actor_heap.rs](plenty-runtime/src/actor_heap.rs) has x86 assembly for nonlocal trap recovery and x86-only heap support. [accounting.rs](plenty-runtime/src/accounting.rs), file abandonment and crypto allocation tests contain related gates. |
| Other gated facilities | [watch.rs](plenty-runtime/src/aggregates/watch.rs) returns unsupported outside x86 Linux although inotify exists on ARM Linux. Search all architecture gates, not just compiler code. |
| Objects and diagnostics | [library_metadata.rs](src/library_metadata.rs) explicitly rejects non-x86 objects. [dump.rs](src/codegen/dump.rs) decodes final bytes with iced-x86. [relax.rs](src/codegen/relax.rs) rewrites x86 GOT loads but already filters by x86 relocation kind. |
| Optimization | [portable_simd.rs](src/codegen/portable_simd.rs) has a scalar fallback outside x86. Preserve correctness without making ARM SIMD optimization a prerequisite. |
| Delivery | [release scripts](scripts/release/), [CI](.github/workflows/ci.yml), [release workflow](.github/workflows/release.yml) and [engine builder](scripts/build_native_engine.py) contain x86 CPU flags, names, tools, loader paths and cache keys. |

## Cranelift guidance from upstream research

1. **Separate backend availability from target selection.** The
   [ISA construction guide](https://docs.rs/cranelift-codegen/latest/cranelift_codegen/isa/)
   describes `isa::lookup(triple)` and shared versus architecture-specific
   flags. In pinned 0.137.0, the AArch64 Cargo feature is named `arm64`;
   `host-arch` is enabled by default. A native ARM build should select its
   backend naturally. An x86-hosted AArch64 codegen probe needs `arm64`
   explicitly. Enabling it does not provide a linker, sysroot or ARM runtime.
2. **Keep a reproducible CPU baseline.** The
   [native builder source](https://docs.rs/cranelift-native/latest/src/cranelift_native/lib.rs.html)
   shows that `builder()` enables host feature inference, whereas
   `builder_with_options(false)` avoids it. Plenty already uses the latter.
   Retain that choice; inspect and record ARM settings instead of enabling
   whatever a developer's CPU supports.
3. **Separate internal Tail ABI from external C ABI.** Upstream
   [calling-convention documentation](https://docs.wasmtime.dev/api/cranelift/prelude/isa/enum.CallConv.html)
   calls Tail non-stable. Keep native C adapters at every foreign boundary;
   never expose Tail as a stable library ABI. Derive the native convention
   from the ISA while preserving Tail for Plenty calls.
4. **Keep inline stack probes.** Local 0.137.0
   `cranelift-codegen/src/isa/aarch64/abi.rs` implements
   `gen_inline_probestack`; its out-of-line `gen_probestack` is unimplemented.
   Plenty already requests inline probes. Do not switch strategy or disable
   protection to get an ARM build through.
5. **Inspect emitted code, not x86 assumptions.** Pinned
   `isa/aarch64/inst/emit.rs` emits `Inst::Udf` for recorded traps. Verify
   delivered signals, fault PC and helper-entry state experimentally; do not
   assume every ARM trap is a debugger breakpoint or that it pushes a return
   address. Inspect ELF relocations as well as CLIF.
6. **Reduce failures to small CLIF cases.** Upstream's
   [fuzzing guide](https://docs.wasmtime.dev/contributing-fuzzing.html) describes
   differential testing and Cranelift fuzz infrastructure. For this port,
   preserve the input, target flags and failing CLIF; compare debug/release
   behavior and C/Rust ABI fixtures before changing backend versions.

Online `latest` and Wasmtime development docs can move. The local registry's
0.137.0 source is the authority for implementation details above. No dependency
upgrade is required by the evidence gathered so far.

## Work packages and acceptance gates

These are proposed reviewable changes, not estimates or assigned issues.
Split a package further when it becomes large; record dependencies in local PRs.

### P0 — Establish an ARM execution harness and backend probe

**Scope:** test/development harness; no production target enablement.

Establish a repeatable QEMU environment, followed by access to a native ARM
runner. Record compiler, libc, kernel, page size, CPU model and tool versions.
Use a small isolated Cranelift 0.137.0 probe to emit ARM ELF, with explicit
`arm64` support when built on x86. Exercise C calls, `u128` arguments/results,
direct/indirect Tail calls with spilled arguments, large frames with inline
probes, data relocations and a recorded trap. Link and execute the probe with
matching ARM tools. This tests the backend without first porting all of Plenty.

**Done when:** probe results and reproducible commands are recorded; backend
failures have minimal reproductions; we have a concrete build/run environment.
Do this before estimating the runtime port.

### P1 — Define the two supported Linux target descriptions

**Scope:** toolchain/ISA setup, target facts, runtime identity, cache contracts.
**Depends on:** P0 evidence.

Centralize the two triples, object architecture, C convention and CPU baseline.
Use that description in validation, version output, metadata inspection and
tests. Keep compiler/ISA/runtime matching mandatory. Audit cache and package
artifact keys for target, CPU features, ABI/runtime changes and compiler version.
Keep public ARM admission gated until the safety-critical runtime work passes.
Avoid a general Windows/macOS abstraction framework in this change.

**Done when:** target descriptions and mismatch tests pass on x86; ARM probe
configuration is consistent; unsupported/mixed targets fail early; cache entries
cannot cross architectures or survive incompatible ABI changes.

### P2 — Build both embedded runtimes and native dependencies for ARM

**Scope:** `build.rs`, native build helpers, runtime Cargo configuration and vendors.
**Depends on:** P0–P1.

Choose between a target-aware direct build helper and an orchestrated runtime
Cargo build in a separate output directory. Prefer upstream build-script logic
where practical, but prove ordinary `cargo build`, offline builds and embedding
still work before committing to that restructuring. Never recurse into Cargo
using the outer build's locked target directory.

Distinguish Cargo HOST from TARGET, and resolve matching C compiler, assembler,
archiver and sysroot. Native ARM builds are sufficient for release; cross-builds
are useful for QEMU iteration. Cover ring's ARM C/assembly files and generated
configuration, BLAKE3, SHA/cpufeatures/libc/getrandom dependency edges, SQLite,
Brotli, zlib, mimalloc, test hooks and the fuzz driver. Reproduce upstream
feature/build-script behavior if direct rustc remains. Preserve deterministic
archives, path remapping, panic-abort, native link dependencies, allocation-test
features and both application/library runtime modes.

**Done when:** compiler and runtime archives contain only matching architecture
objects; clean locked builds work; embedded and workspace runtime dependency
behavior agrees; crypto/compression known-answer tests pass; x86 remains green.
Credit any newly shipped source/data in module headers and notices.

### P3 — Correct Linux bindings, filesystem flags and synchronization

**Scope:** runtime OS boundary, `threads.rs`, `mutex.rs`, readiness/watch/signals.
**Depends on:** P2 for ARM execution.

Replace copied architecture-dependent constants/layouts with reviewed target
bindings or narrowly scoped target modules. Prefer existing libc definitions
over another independent copy, accounting for the direct runtime build.
Use native C size/alignment/offset/constant probes to validate manual bindings.
Audit all `extern "C"` declarations and raw syscalls, not only current cfg gates.

Correct futex numbering and filesystem flags; validate pthread storage, epoll,
inotify, sockets, process and directory interfaces. Preserve rooted-directory
no-follow guarantees and allocation-failure behavior. Audit generated atomic
operations and Rust synchronization for ARM's weaker memory ordering, including
mutex fast paths, reference counts, channels and published runtime state.

**Done when:** contention/wakeup tests, filesystem symlink/race tests, signal
delivery, watcher/readiness, process and network tests pass on ARM. Tests verify
semantics rather than merely asserting successful system calls.

### P4 — Port checked traps and stack-overflow handling

**Scope:** runtime traps/stack overflow plus compiler trap tables and adapters.
**Depends on:** P0, P2–P3.

Introduce architecture-specific context access and helper-entry restoration.
Preserve fault-site messages, source locations, exit codes and chaining of
unrelated signals. Account for ARM PC/SP/link-register and stack alignment;
validate alternate signal-stack capacity, including extended register state.
Check runtime page size and qualify supported page-size configurations; do not
assume x86 page-fault error bits or a red zone.

Exercise main/worker stacks, deep recursion, oversized frames, unrelated faults,
preinstalled handlers, and embedded-library hosts. Library/fuzz builds currently
use helper calls instead of signal traps: retain their contracts. A deliberate
helper-call path may bootstrap ARM checks, but any temporary difference must
be recorded and tested rather than silently losing diagnostics or protection.

**Done when:** `test_trap_locations` and `test_stack_overflow` equivalents pass
under ARM debug/release, including negative signal-coexistence cases. Stack
probes stay enabled. Full-system QEMU/native results corroborate user-mode tests.

### P5 — Qualify language lowering, C ABI, objects and assembly dumps

**Scope:** codegen, FFI adapters, metadata, dumps and related test expectations.
**Depends on:** P1–P4 for complete execution.

Audit all native signatures and callbacks: startup, runtime ops, generators,
sorting/executors, threads, imports, exports and indirect calls. Test small
integer extension, booleans, floats, `i128/u128`, aggregates, hidden return
storage, stack arguments and alignment against compiled C/Rust fixtures.
Exercise closures, generators, tail recursion and register-pressure cases.

Admit AArch64 ELF metadata while still rejecting wrong architectures and ABI
contracts. Validate PIC/PIE, static/shared exports, C headers, library discovery,
residency, symbol retention and relocations. Preserve the existing x86 relaxation
filter; prove ARM code is not rewritten. Add ARM assembly decoding for `--dump
asm` with accurate offsets/relocations, or explicitly gate it in a development
preview until implemented. Do not feed ARM bytes to iced-x86.

**Done when:** FFI/library/Tail suites pass, ARM dump output is meaningful,
debug/release outputs agree, and cold/warm/cache-off compilation behaves equally.
`std.simd` scalar fallback passes; vector optimization remains optional.

### P6 — Port actor heaps and nonlocal trap recovery

**Scope:** actor heap backend, allocator routing, recovery assembly and cleanup.
**Depends on:** P3–P5.

Implement an AAPCS64 recovery frame preserving the required general-purpose and
FP/SIMD callee-saved state, SP and return state. Validate assembly independently
before enabling actor APIs. Audit mapping alignment, page sizes, virtual-address
reservation, allocation routing/accounting and file abandonment. Update the
related cfg gates together. Preserve trap containment without Rust unwinding.

**Done when:** actor isolation, memory limits, trap kinds, repeated recovery,
supervision, file cleanup, actor networking/TLS and allocation-injection tests
pass. ARM cannot be advertised as full standard-library parity while actor
operations remain the existing unsupported fallback.

### P7 — Enable ARM and qualify the complete developer workflow

**Scope:** target admission, integration tests, packages/cache/LSP, CI.
**Depends on:** P2–P6.

Enable the target after required gates pass. Run the workspace with all features
on both architectures, plus default-feature builds, doctests, formatting and
clippy. Update x86-specific expectations in target-conditionals, toolchain, dump,
SIMD, size and relocation tests without weakening behavioral coverage.
Qualify `plentyc` and `plenty`, package build/run/test, runtime extraction,
distribution assembly, LSP, parallel compilation and paths containing spaces.
Keep ARM/x86 caches and artifacts distinct. Run tutorial/reference examples.

Qualify libFuzzer/tool discovery and sanitizers separately where installed;
report unavailable developer facilities explicitly. Run network/proxy tests
in environments with the required services. Every skip needs a recorded reason.

**Done when:** native ARM CI is a required regression gate, x86 CI passes,
and no standard feature silently degrades to an unsupported stub.

### P8 — Qualify optional dataframe and linalg engines

**Scope:** engine crates, `build_native_engine.py`, engine packaging and tests.
**Depends on:** P5; final qualification also needs P7.

Remove host/CPU assumptions from engine tooling, audit Polars/faer and native
dependencies for baseline ARM support, and verify engine ABI/architecture checks.
Build matching shared objects and exercise engine integration and tutorial tests.
Preserve the existing behavior when an engine is absent.

**Done when:** qualified ARM engine artifacts work with the matching compiler,
or the release explicitly states those optional packages are unavailable.
Engine availability is a separate release decision, not a reason to disable
the core port or claim untested engine support.

### P9 — Release artifacts and support documentation

**Scope:** release scripts/workflow, installation/support/reference/tutorial docs.
**Depends on:** P7, and P8 for each engine shipped.

Parameterize target names, CPU flags, build image/digests, rustup/mold downloads
and checksums, ELF machine/loader checks, dependency allowlists, notice filtering,
archive selection and smoke-test assertions. Retain x86 artifacts. Do not assume
every existing smoke-test distribution/image supplies ARM64 builds.
Audit compiler binaries, both embedded runtimes, generated programs and engines
against the chosen glibc/CPU baseline. Confirm byte reproducibility within each
target's fixed build environment, not equality between different architectures.

Smoke-test extracted archives in clean ARM environments with only documented C
linker prerequisites, no Rust/Cargo and no network. Test the oldest supported
environment and a current distro. Update supported targets, baseline/version
output, installation, linking, actor/SIMD limitations, release instructions and
backlog together. Run all prescribed docs CI checks before those PRs are handed
off. Actual publication remains a separate user-authorized action.

**Done when:** independently audited ARM archives compile and run representative
programs, checksums/notices are complete, and docs state the tested contract.

## QEMU development path at the research baseline

This workstation is x86-64. Inspection found `qemu-aarch64`,
`qemu-aarch64-static`, `qemu-system-aarch64` and the Rust
`aarch64-unknown-linux-gnu` target installed. Neither `aarch64-linux-gnu-gcc`
nor `aarch64-linux-gnu-ld` was found on PATH, and `/usr/aarch64-linux-gnu` was
absent. This does not exclude toolchains/root filesystems elsewhere. No guest
or ARM test was run during planning.

Use two complementary routes:

1. **Fast user-mode probes:** obtain a pinned ARM GNU sysroot and cross C tools;
   cross-build isolated probes and, after P2, the compiler. Execute an ARM
   binary with `qemu-aarch64 -L /path/to/arm-sysroot ./arm-program`. A Cargo target
   runner can run Rust tests, but does not automatically wrap every executable
   spawned by Plenty's integration tests. Arrange explicit subprocess wrappers
   or a verified binfmt/container environment; ensure Plenty invokes an ARM
   linker rather than host `cc`. Avoid changing global binfmt registration as
   an invisible harness prerequisite.
2. **Complete ARM guest:** boot a GNU/Linux image with `qemu-system-aarch64`,
   `-machine virt`, an explicit ARM64 CPU such as `cortex-a53`, and TCG on this
   x86 host. Install Rust/C tools inside the guest and run native builds there.
   This avoids adding production cross-compilation just to run integration
   tests and exercises an ARM kernel's signal/page-size behavior. Pin the
   image, kernel, CPU and QEMU configuration. Test additional page sizes with
   corresponding guest kernels when included in the support contract.

QEMU documents both [user-mode syscall/signal translation and memory-model
limitations](https://www.qemu.org/docs/master/user/main.html). Use its syscall
trace and GDB facilities for diagnosis. For whole-system guests, upstream
recommends the [`virt` machine](https://www.qemu.org/docs/master/system/arm/virt.html)
and requires choosing a 64-bit CPU explicitly. Test a baseline CPU rather than
only `max`. QEMU execution is valuable evidence; native ARM runs are still
required for concurrency stress, realistic performance and release qualification.

## Sequence and first implementation session

Critical path: **P0 → P1 → P2 → P3/P4 → P5 → P6 → P7 → P9**.
P8 can follow ABI qualification independently of actor work. Release-script
design can start early, but release claims wait for execution evidence.
Avoid overlapping edits to `build.rs`, codegen signature setup and runtime
signal handling across unrelated PRs.

Start by locating or provisioning a pinned ARM guest/sysroot and running P0.
Then choose the P2 runtime-build approach using the probe evidence. The largest
uncertainties are dependency-build parity, signal/stack handling and actor
recovery; refine estimates after those spikes. A hello-world success is an
early checkpoint, not completion of the port.

Implementation PRs should include focused tests and relevant reference/tutorial
changes. New defects discovered in supported behavior belong in the shared
issues register after duplicate search; claim numbered work atomically and keep
issues open until the fixing PR merges. The original planning-only revision made
no compiler modifications or ARM test runs; subsequent implementation and
emulated evidence are described in
[Linux AArch64 development](book/src/development/linux-aarch64.md).
