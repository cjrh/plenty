# Interface boundaries and source mapping

> Proposed organization, based on Plenty `869e1cb8`. No new target is admitted
> by this design. Work packages and priorities belong to [B18](../../backlog.md).

## Separate three identities

Cargo's **build host** runs build scripts. The **compiler host** runs Plenty and
its linker driver. The **generated-program target** supplies the ISA, ABI,
runtime and system libraries. Native releases currently equate the latter two;
cross-building the Rust compiler does not change that relationship. Preserve
separate identities even while enforcing equality at admission.

Use immutable target facts for triple, OS, architecture, object format, native
calling convention, minimum system version and artifact naming. Derive these
from one checked description; do not duplicate target-name comparisons in the
frontend, cache, package tooling and release scripts. Cache identity must include
runtime/ABI compatibility and features, not just the CPU name.

## Focused interfaces

| Owner | Shared operation | Implementation boundary |
| --- | --- | --- |
| Compiler host/tool invocation | Discover tools; execute an argument vector and report errors | Host process/filesystem APIs; explicit target linker/SDK configuration |
| Target codegen | Build ISA; describe native calls; enumerate special probe sites; decode instructions | Reuse Cranelift `TargetIsa`; small Plenty-specific architecture helpers |
| Object emission and inspection | Retain/read interface records; resolve source/trap records; export symbols | ELF, Mach-O and COFF implementations owning their symbol/section state |
| Linking/distribution | Construct a link plan; locate dependencies; validate an installed engine | Driver-specific argument arrays and format-specific dependency inspection |
| Runtime OS services | Open/close/wait/spawn; obtain entropy; classify errors | Linux, Darwin and Windows service implementations with typed resource ownership |
| Fault recovery | Read saved PC/SP; enter a helper; recover an actor or terminate safely | OS-and-architecture context/exception implementations, selected at build time |
| Plenty standard library | Portable policy, ownership and meaningful results | Native backend normalization; `if const` only at deliberate source boundaries |

A single object combining these responsibilities would couple compiler state to
runtime mechanics. Prefer modules with identical functions for build-time
choices, small traits for genuinely interchangeable behavior, and data for
facts. Keep stateful symbol/relocation work inside the emission component;
passing the entire compiler through a platform trait would defeat the boundary.
No global platform initialization, allocation or dynamic dispatch should be
introduced into signal handlers merely to satisfy this organization.

Plenty protocols provide static constraints where a library genuinely accepts
multiple providers. They need not be threaded through every application to use
files or networking. Platform selection belongs behind public standard-library
interfaces. Protocols do not supply missing OS behavior or dynamic dispatch.

## Compiler changes with concrete owners

| Current location | Dependency to hide | Acceptance evidence |
| --- | --- | --- |
| `src/frontend/target.rs::fact`, `src/toolchain.rs::NATIVE_TARGETS` | Linux is returned directly; table omits OS/object/link facts | Frontend facts agree with runtime manifest and emitted object for every admitted target |
| `src/codegen/output.rs::finish`, `src/library_metadata.rs` | GNU ELF flags and ELF-only metadata reading | Format-aware objects; metadata survives executable/static/shared library dead stripping |
| `src/codegen/exports.rs`, `sites.rs`, `traps.rs`, `coverage.rs` | Section names, relocation and linker retention assumptions | Source/trap/coverage lookup and exported contracts after relocation and installation |
| `src/toolchain.rs`, `src/library.rs`, `src/toolchain/distribution.rs` | GNU options, ELF SONAME/RUNPATH and `.so` names | Driver argument tests plus real link/load/relocation tests with spaces in paths |
| `src/codegen/stack_probes.rs`, runtime `signal_context.rs` | Pinned ARM instruction sequence and Linux context fields | Registered probes only; native overflow and unrelated-fault tests; handler coexistence |
| `build.rs`, engine/release helpers | Direct dependency builds and Linux assembly/tool conventions | Matching compiler/runtime/native objects; reproducible offline build and artifact audit |

Cranelift 0.137 provides `AppleAarch64` and `WindowsFastcall`, and Mach-O/COFF
object emission. That is feasibility evidence, not a working Plenty port. Keep
internal `Tail` calls distinct from native adapters. Test narrow signed/unsigned
arguments, mixed float/integer register exhaustion, u128 and callbacks against
both Rust and the chosen C toolchain. Windows GNU and MSVC are separate cases.

Audit unwind information explicitly: Plenty uses `define_function_bytes`, which
bypasses the object backend's automatic unwind processing. The pinned backend
also has limitations for automatic Windows and Mach-O unwind emission. Retaining
panic-abort does not settle OS exception/stack-walking requirements. Native stack
probes, handler coexistence and actor recovery are separate admission gates.

## Runtime and language contracts

| Plenty surface / current dependency | Reference implementation to inspect | Proposed boundary and required tests |
| --- | --- | --- |
| `files.rs`, `aggregates/filesystem.rs`; Unix descriptors, no-follow, temporary publication | Rust `sys/fs/windows.rs`; CPython `Modules/posixmodule.c`, `Python/fileutils.c` | Typed resources and rooted operations; rename/link races, sharing, delete-open behavior, atomic publication and confinement |
| `stdlib/std/path.plenty`, environment and arguments; native Unix bytes | Rust `sys/os_str`, `sys/process/windows.rs`; CPython `Lib/ntpath.py`, `_winapi.c` | Lossless native names and explicit conversions; UNC/drive paths, unpaired wide units, invalid Unix UTF-8, Unicode and spaces |
| `aggregates/readiness.rs`, `mutex.rs`, cancellation; epoll/eventfd/futex | Rust `sys/sync`, `sys/net`; CPython `Modules/selectmodule.c` | Wait/wake contracts covering sockets, pipes and cancellation; lost wakeups, EOF, stale registrations, timeout and handle reuse |
| `stdlib/std/fs/watch.plenty`, `aggregates/watch.rs`; inotify records/masks/cookies | CPython selector backends help waiting, but do not supply equivalent watch semantics | Normalize events below portable policy; decide rename pairing, overflow and recursive behavior without inventing guarantees |
| `aggregates/net.rs`, `stdlib/std/net.plenty`, `report.plenty`; Unix handles and numeric errno | Rust `sys/net/connection/socket/windows.rs`; CPython `Modules/socketmodule.c` | Distinguish socket ownership and OS error domains; partial I/O, would-block, DNS lifetime, SIGPIPE and cancellation |
| `aggregates/process.rs`, `files.rs`; spawn/pipes/inheritance | Rust `sys/process/windows.rs`; CPython `Modules/_winapi.c`, `_posixsubprocess.c` | Literal arguments, explicit inherited handles, bounded pipe draining, missing executable, timeout, termination and reap |
| `threads.rs`, `actor_heap.rs`, `signals.rs`, `stack_overflow.rs` | Rust `sys/thread/windows.rs`, `sys/pal/{unix,windows}/stack_overflow*` | Fallible startup without hidden allocation; native fault/exception recovery, cleanup, shutdown notifications and concurrent first use |
| `entropy.rs`, crypto/TLS/compression dependencies | Rust `sys/random/{apple,windows}.rs`; CPython `Python/bootstrap_hash.c` | Fallible secure entropy, no weak fallback; known-answer tests and CPU-dispatch checks |

Reference paths are relative to the read-only checkouts and revisions recorded
in [delivery](delivery.md). They identify patterns and tests to study, not code
licensed for uncredited copying. Prefer Rust facilities where their fallibility,
allocation and progress semantics fit; borrowed implementation code/data needs
its license and attribution handled explicitly.

The most consequential API decisions are Windows native path representation,
readiness versus completion, filesystem event guarantees, OS error domains and
shutdown signals. Record these before implementing backends. Security guarantees
must survive the port; an unavailable operation must fail explicitly instead of
silently following symlinks, losing filename data or weakening entropy.

## Contracts requiring decisions before mechanical refactoring

`stdlib/std/process.plenty` encodes normal exits as 0–255 and POSIX signals as
256 plus the signal number. Windows native 32-bit exit codes require a distinct
representation; preserve full status and distinguish termination from normal
exit. Startup argument capture in `text_io.rs` also assumes C `argc/argv` bytes;
compare Rust's `GetCommandLineW` handling before choosing Windows behavior.

The generated mutex fast path assumes a u32 state word with states 0/1/2, while
the runtime promises no allocation. Rust's Darwin mutex implementation uses
pthread primitives, and its Darwin thread parking uses public libdispatch
semaphores rather than private address-wait APIs. A direct substitution must
therefore prove layout, initialization and allocation contracts or change the
Plenty representation/lowering deliberately. Rust's Windows implementation uses
address-based waiting; establish the supported Windows baseline before reuse.

`control::Token` stores a Unix `OwnedFd` even outside Linux-only operations.
Separating function bodies alone will not make Windows compile: resource
representations must also become platform-specific. Filesystem watching needs
an explicit choice between retaining the Linux-native API and introducing a
portable change/rescan contract; equivalent-looking notifications are not proof
of equal rename, overflow or event-order guarantees.
