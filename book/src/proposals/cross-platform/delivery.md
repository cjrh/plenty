# Qualification, delivery, and reference implementations

> Part of the [cross-platform assessment](../cross-platform.md).
> These are dependency gates and design criteria; priorities remain in B18 of
> the [backlog](../../backlog.md), not a second work queue.

## Evidence required before calling a target supported

| Gate | Prerequisites and acceptance evidence |
| --- | --- |
| Native backend experiment | Build an object, link and execute C/runtime calls, aggregate returns, indirect Tail calls, large frames and library metadata on the exact OS/ISA. Probe ARM64 COFF relocations before promising Windows ARM64. |
| Target/build boundary | Compiler and both embedded runtimes report matching target/ABI; target-aware native dependencies and drivers work offline; incompatible targets are rejected early. Linux x86-64 remains green. |
| Runtime core | Allocation failures, reporting, stack guards/overflow, thread start failure/join, channels, atomics, files and foreign ABI round trips pass natively. No unsafe success stubs. |
| OS and stdlib | Native paths and resources, rooted filesystem guarantees, process cleanup, cancellation/readiness, DNS, TLS and HTTP pass the platform capability contract. |
| User tooling | Package build/run/test, cache races, LSP path round trips, static/dynamic libraries and supported fuzz tooling are exercised from an extracted release. |
| Distribution | Fresh machine with documented native-linker prerequisites, no Rust/Cargo and no network compiles and runs representative programs; dependency/OS baseline audits and signed-package policy pass. |

The Linux AArch64 experiment can retain Linux semantics while exposing ABI,
native assembly and memory-ordering assumptions. Darwin ports then exercise
Mach-O, Apple ABI and Unix OS differences. Windows follows the decisions about
paths, handles and readiness; Windows ARM64 depends on both the Windows port
and backend feasibility. OS implementations can proceed independently once the
shared contracts settle. Native release jobs must wait for their corresponding
qualification gates; a compiler that prints `--version` is not enough.

## CI and regression strategy

Add native test runners for each admitted tuple, with explicit toolchain/SDK and
minimum-version policy. Emulators and cross-compilation can supplement coverage
but do not replace real OS execution for thread scheduling, sockets, file races,
signals, DLL loading or installer behavior. Runner availability and cost are
implementation-time decisions; this assessment did not provision machines.

Inventory Linux-specific fixtures: `/proc/self/fd`, `/tmp`, shell scripts,
descriptor counts, errno numbers, ELF tooling, chmod, signals, executable suffixes
and symlink privileges. Keep portable behavioral tests common and replace
platform observations with equivalent checks. For example, verify resource
return to an observed baseline, using descriptor counts on Linux and qualified
handle accounting on Windows. Preserve adversarial leak/race/security tests;
do not just skip every failure under a broad non-Linux condition.

Run compiler and runtime workspace suites with all features, tutorial examples,
allocation injection and relevant release-helper tests. Test debug and release
generated programs, cache on/off, library hosts and parallel execution. Include
non-ASCII and non-text native names, paths with spaces, inheritance leaks,
timeouts, repeated cancellation, DNS cleanup, TLS handshake errors and HTTP
shutdown. Keep environment-dependent Unicode/version assertions semantic rather
than tied to incidental toolchain output.

## Release packaging and user prerequisites

Extend the existing tag-verified, locked, draft-then-publish workflow to build
one archive per admitted tuple. Preserve tag/version checks, immutable published
artifacts, checksums, licenses and third-party notices; the notice generator
currently fixes its dependency target to Linux x86-64. **No crates.io publishing**
is needed or proposed. Keep build, test and publication permissions separated.

Retain `.tar.gz` on Unix; use `.zip` on Windows, with `plenty.exe` and
`plentyc.exe`. Update packaging/smoke helpers that assume one Linux suffix or
archive glob. Every archive must contain target-specific installation guidance
and the same required resources/notices. Test extraction and PATH setup as an
ordinary user; `.exe` naming must propagate into packages and test runners.

For Linux AArch64 select and verify a glibc baseline and dynamic-loader/dependency
allowlist, rather than reusing x86-64 ELF paths. For macOS choose deployment
targets, audit Mach-O architectures/install names and linked libraries, and
test Apple Command Line Tools/SDK discovery. For Windows choose supported OS,
SDK and CRT requirements; audit PE imports and run from a clean environment
with only the documented native tools. The compiler still needs a native
linker to build programs; bundling a compiler driver/SDK would be a distinct
distribution/licensing decision.

Decide macOS Developer ID signing/notarization and Windows Authenticode policy
before public downloads. Credentials, account access and signing runners are
prerequisites for that release stage, not assumptions this proposal fulfills.
Separate unsigned reproducible build checks from signed-artifact verification
and document timestamp/signature differences. Test macOS quarantine/Gatekeeper
and Windows download/launch behavior; signing is not a promise that every
reputation prompt disappears. A preview must state unsigned status honestly.

## Inspected reference implementations

The local checkouts were used read-only, without copying their implementations.
Paths below are relative to the named checkout, making them useful even when
the repos are not adjacent to a reader's Plenty checkout. These snapshots are
design evidence, not claims about the latest upstream support policy.

**Rust**, `../rust/`, commit
`080cc27146389dbb5c8772c476b1937be9c6110f`:

| Inspected path | Transferable lesson |
| --- | --- |
| `library/std/src/sys/pal/mod.rs` | Select platform implementations beneath a shared interface |
| `library/std/src/sys/fs/windows.rs` | Wide-path file APIs and owned handles, rather than Unix descriptor emulation |
| `library/std/src/sys/process/windows.rs` | Native UTF-16 environment/command handling and argument quoting require explicit policy |
| `library/std/src/sys/net/connection/socket/windows.rs` | Winsock resource ownership and socket errors differ from file handles |
| `library/std/src/sys/thread/windows.rs` | Native entry ABI, startup failure cleanup and join ownership; do not copy its boxed allocation strategy blindly |
| `library/std/src/sys/pal/unix/stack_overflow/{mod,handler_signal}.rs`, `library/std/src/sys/pal/windows/stack_overflow.rs` | Stack protection/diagnostics need OS-specific setup and handler coexistence |
| `library/std/src/sys/random/{apple,windows}.rs` | Entropy source selection depends on platform; adapt to Plenty's fallible contract instead of copying panic behavior |
| `compiler/rustc_target/src/spec/targets/{aarch64_apple_darwin,aarch64_pc_windows_msvc}.rs` | Architecture, ABI, baseline and atomic capabilities belong in target descriptions; rustc support does not prove Cranelift coverage |

**CPython**, `../cpython/`, commit
`b3714a5fb7a3aadd72134ee0710b22519c1df09a`:

| Inspected path | Transferable lesson |
| --- | --- |
| `Modules/posixmodule.c`, `Python/fileutils.c` | File operations and lossless native names need platform boundaries; Python's surrogate policies are precedents, not a requirement to alter Plenty strings |
| `Modules/socketmodule.c` | Separate socket closing/error behavior and platform capability detection |
| `Modules/selectmodule.c` | poll/epoll/kqueue and Windows socket selection are different capabilities; one selector call cannot cover every resource |
| `Modules/_winapi.c` | Windows process/pipe/handle mechanics deserve a dedicated implementation |
| `Python/bootstrap_hash.c` | OS entropy paths, size limits and failure handling; never weaken randomness for portability |
| `Lib/ntpath.py` | Drive, UNC and native path rules need a distinct model from POSIX slash operations |

These references show patterns to adapt within Plenty's ownership, bounded
resource and explicit-allocation rules. They do not license assuming equal
semantics across systems. No Rust or CPython changes are proposed.

## Decisions still open

Before implementation, settle the supported OS/CPU baselines, Windows native
path API migration, readiness/completion bridge, and reduced-preview capability
policy. Then choose runtime build orchestration, target-specific tool discovery,
and the scope of static/dynamic library support at first release. Confirm ARM64
COFF behavior with an executable experiment. Signing credentials and native CI
capacity can be arranged in parallel, but are required before qualified releases.

The highest uncertainty is Windows resource/security semantics and backend
coverage, followed by maintaining fallible allocation across portable helpers.
The lowest-risk first increment is Linux AArch64 after target-aware dependency
builds and signal/stack ABI corrections. No effort estimate is reliable until
those focused experiments establish the actual backend and SDK constraints.
