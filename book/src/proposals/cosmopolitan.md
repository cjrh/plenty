# Cosmopolitan and Actually Portable Executables

> **Status: Not implemented.** A bounded feasibility spike demonstrated two
> programs, not supported Cosmopolitan output. This proposal recommends an
> evidence-gated compatibility experiment before a platform commitment.
> Current behavior remains in [runtime packaging](../design/toolchain/02-architecture/runtime.md)
> and [linking](../design/toolchain/01-building/linking.md). Future work belongs
> to B18/B17 in the [backlog](../backlog.md).

Cosmopolitan offers a different deployment route from the
[native-platform assessment](cross-platform.md): generated programs could use
Cosmopolitan Libc and the Actually Portable Executable (APE) container to run on
multiple operating systems. It does not automatically make Plenty's GNU/Linux
runtime portable. Keep native GNU/Linux output unchanged and make any eventual
Cosmo output explicitly opt-in.

The recommendation is **not** to implement a broad runtime port now. First
establish a reproducible x86-64 runtime compatibility result, including failure
paths, threads and execution of the identical artifact on a non-Linux OS.
Then choose a restricted feature profile, a larger runtime port, or no support.
The [runtime design](cosmopolitan/runtime.md) describes the unresolved build and
ABI choices; [evidence and qualification](cosmopolitan/evidence-and-gates.md)
records what was actually measured and the go/no-go criteria.

## Multi-OS is not multi-architecture

The spike used `x86_64-unknown-cosmo-cc`: its successful Plenty artifacts contain
x86-64 code only. Multi-OS output on one ISA is useful independently of fat
output. The ordinary `cosmocc` C workflow compiles both x86-64 and AArch64 and
combines the results with `apelink`; it cannot manufacture AArch64 Plenty code
from an x86-64 object. True fat Plenty output requires two qualified codegen
paths, two compatible runtimes and dependencies, and packaging and testing of
both slices.

Native Linux AArch64 work is being developed independently (local PR #706,
not merged into the inspected `main`). Coordinate target descriptions, ABI
checks, platform boundaries and tests with that work when available. Do not
copy unmerged code or treat its Linux GNU runtime, signal context or release
qualification as Cosmopolitan support. Native Windows/MSVC and Darwin ABIs are
also different targets from the C ABI offered by Cosmo.

## Support matrix and deployment contract

This matrix separates upstream scope from Plenty evidence. It is not a support
announcement or a promise of every OS × ISA combination.

| Environment / artifact | Evidence | Proposed admission |
| --- | --- | --- |
| Native Linux x86-64 GNU | Implemented baseline at inspected main `2851ef34` | Preserve existing support |
| Cosmo x86-64 on Linux | Empty and print/allocation programs run; checked trap fails | Compatibility experiment only |
| Same Cosmo x86-64 artifact on Windows, macOS or BSD | No Plenty execution tested | One real non-Linux OS required for experiment; each named OS/version needs separate qualification |
| Cosmo AArch64 on any OS | No Plenty evidence | Separate ABI/runtime feasibility gate |
| Combined x86-64 + AArch64 APE | No Plenty evidence | Only after both slices qualify independently |
| BIOS / bare metal | Upstream release mentions BIOS; no Plenty evidence | Non-goal for this proposal |

The [official toolchain documentation](https://github.com/jart/cosmopolitan/blob/4.0.2/tool/cosmocc/README.md)
names Linux, macOS, Windows, FreeBSD, OpenBSD and NetBSD and describes the two
ISA toolchains. These upstream claims do not establish Plenty's OS versions,
CPU baseline, capabilities or deployment guarantees. Choose and record those
per qualified configuration rather than inheriting an upstream support list.

APE needs a working execution route. On the spike host a shell launched the
successful programs, but direct Python subprocess execution returned `ENOEXEC`.
The official documentation describes shell fallback, loader extraction into
`TMPDIR`/`HOME`, optional system loader/binfmt configuration, and Apple Silicon
loader installation/build requirements. Test writable/executable extraction
locations, restricted environments and installed-loader operation explicitly.
Do not require privileged registration by default or silently alter the host.
Do not claim every deployment is a zero-prerequisite direct `execve` launch.
Windows shell requirements for the toolchain wrappers are not a claim that
every final Windows executable needs a POSIX shell.

The compiler should initially remain a native GNU/Linux tool producing an
experimental artifact. Porting the compiler executable itself to APE, making
all native compiler hosts cross-compile Cosmo, and fat output are separate
questions, not prerequisites for the bounded experiment.

## Alternatives and non-goals

- **Keep native ports only.** Lowest additional maintenance; loses single-file
  multi-OS deployment. Prefer this if compatibility or qualification gates fail.
- **Linker override alone.** Useful for investigation, rejected as a supported
  design: the spike already demonstrates a trap failure and ABI mismatch.
- **Restricted Cosmo runtime.** Recommended candidate after the experiment;
  explicitly admit a small tested capability set instead of advertising the
  whole standard library. This still requires correct runtime fundamentals.
- **Broad runtime and dependency port.** Potentially useful, but only justified
  after measuring Rust std compatibility and concrete module/dependency costs.
- **Dedicated Rust/Cosmo target or reduced runtime without Rust std.** Could
  remove GNU assumptions, but carries substantial toolchain/runtime maintenance;
  compare against selective compatibility shims using evidence, not ideology.

This proposal does not replace Cranelift, change Plenty semantics, weaken
fallible allocation/security contracts, implement native MSVC/Darwin ports,
provide BIOS services, promise arbitrary dynamic C libraries, or bundle a new
SDK into every compiler distribution. It supplies no production effort estimate.

## Decision and risk policy

The major risks are latent GNU Rust std assumptions, signal/context corruption,
TLS and synchronization bugs, incompatible native dependencies, and insufficient
real-OS runners. A tiny program's dead-section elimination can hide all of them.
Loader policy and toolchain supply-chain maintenance add deployment risk even
when program semantics pass. A restricted profile reduces exposed facilities,
not the need to establish memory safety and thread lifecycle correctness.

Decisions about the final runtime build model, admitted capabilities, minimum
OS/CPU versions, public option spelling and toolchain redistribution remain
behind the [qualification gates](cosmopolitan/evidence-and-gates.md). Passing a
gate permits a design decision; it does not silently authorize broader support.
