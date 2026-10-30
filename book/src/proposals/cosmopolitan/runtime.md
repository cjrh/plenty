# Cosmo target, runtime and driver design

> Part of the [Cosmopolitan proposal](../cosmopolitan.md). Recommendations here
> are not implemented behavior; build choices remain subject to evidence gates.

## Separate target identity from the final linker

Current `src/toolchain.rs::validate_target_pair` accepts only the packaged
`x86_64-unknown-linux-gnu` target. `src/codegen.rs` builds a host ISA and uses
System V at exported runtime boundaries. `build.rs` builds and embeds Rust
static archives for Cargo's target; selecting `--linker` does not change those
compile-time assumptions. See [current runtime packaging](../../design/toolchain/02-architecture/runtime.md).

An eventual target descriptor should distinguish compiler host, generated ISA,
Cosmo C ABI, ELF object format, APE final container, runtime profile and toolchain
identity. Do not describe the generated program as GNU/Linux merely because the
compiler runs there. Preserve the internal Plenty Tail convention separately
from Cosmo C calls. Verify entrypoints, helpers, callbacks, aggregate returns,
i128/libcalls, stack alignment, atomics, relocations and direct/indirect Tail
calls; familiar ELF and x86-64 are not a complete ABI proof.

For the experiment, retain object/runtime extraction and external link harnesses
rather than prematurely committing to a public target spelling. For production,
choose an explicit output-target option integrated with existing target selection;
`--linker` alone must never imply support. Host execution of a generated APE is
a separate operation from compilation, with a documented loader selection policy.

## Runtime build choices to resolve experimentally

Do not assume linking GNU-built Rust std into Cosmo is generally safe. Rust
Linux cfgs, libc layouts, TLS, entropy, filesystem and thread implementations
can survive final linking unchanged. Conversely, the successful allocation probe
is evidence against declaring every GNU object unusable or requiring every
dependency to be rebuilt without inspection.

Compare these bounded choices:

| Candidate | Required evidence / cost |
| --- | --- |
| Retain selected GNU-built Rust components with Cosmo-header C shims | Audit reachable std/dependency paths and generated code; prove no incompatible OS layouts/syscalls/TLS assumptions escape the boundary |
| Build a dedicated Rust/Cosmo runtime configuration and compatible dependencies | Establish target/std availability or maintained patches, build-script behavior, ABI, reproducible/offline builds and ownership of long-term maintenance |
| Reduce OS-facing Rust std use in a restricted runtime | Measure how much replacement is actually necessary; preserve allocation, panic, reporting and thread contracts rather than invent a second language runtime |

The first candidate is an experiment, not an endorsed distribution architecture.
A shim fixes bindings it owns, not arbitrary std internals. If those internals
cannot be bounded, stop or move to an explicitly approved runtime build project.
Reconcile the chosen build with the [native runtime build assessment](../cross-platform/compiler.md),
including separate output directories and consistent application/library modes.
Do not recursively invoke Cargo against the enclosing build directory.

## A narrow platform boundary

Prefer C compiled against pinned Cosmo headers where the libc ABI is complex or
has runtime-valued constants. Expose small fixed-width/pointer interfaces to Rust;
keep platform structs private to C, with build-time size/alignment and native
round-trip checks. This is justified by ABI translation, not a reason to wrap
ordinary Plenty library algorithms.

| Boundary | Required treatment |
| --- | --- |
| Signals and saved contexts | Replace GNU `sigaction`, masks, flags, alternate-stack and register-context assumptions in `traps.rs` and `stack_overflow.rs`; install/chaining/restore and saved-PC/SP access belong behind the boundary |
| Trap reporting and stack guards | Preserve source-site diagnostics and exit semantics, inline page probes and main/worker alternate-stack lifecycle; distinguish owned traps/guard faults from foreign faults |
| TLS and threads | Audit Rust TLS lowering plus Cosmo fixups, initialization/destructors, pthread entry/attributes, stack bounds, join and start-failure cleanup; test per-thread allocation/trap state isolation |
| Synchronization and syscalls | Replace or validate `mutex.rs`'s raw Linux syscall 202/futex assumptions; use qualified Cosmo primitives, testing wakeups, contention, cancellation and memory ordering |
| Filesystem and processes | Audit `filesystem.rs` statx/readdir64 layouts, flags, paths, errors, rooted operations and process inheritance/cleanup; do not substitute unsafe path traversal for secure operations |
| Network, readiness and watch | Audit pthread attributes in `net.rs`, sockets, DNS, clocks, nonblocking/error semantics and Linux inotify/readiness bindings; admit capabilities individually |
| Foreign libraries | Existing dlopen/ELF residency/metadata assumptions in `libraries.rs` are not an APE portability contract; reject until independently specified and qualified |

Signal-handler work must include async-signal-safety, previous-handler coexistence
and unknown-fault forwarding. Prefer existing helper-call checked traps as an
explicit experimental fallback if necessary, but still test the signal path;
disabling stack guards is not an acceptable fallback. Libraries must continue
to respect host handlers. Actors' recovery and per-actor heaps need their own
qualification rather than inheriting success from a main-thread trap.

Cosmo's TLS fixups may adapt eligible GNU instruction patterns; the spike
observed GS accesses, not a TLS failure. Neither that observation nor an isolated
thread-local read proves Rust TLS destructor and worker lifecycle compatibility.
Test scoped workers, executors, repeated spawn/join, foreign-thread entry if
admitted, thread-local failure injection and trap caller-site reset between jobs.

## Capabilities and dependencies

A candidate initial profile is executable-only core language, collections,
allocation, reporting and basic standard output. Threads must be tested in the
compatibility experiment even if public concurrency is withheld initially.
Filesystem, processes, network/TLS, watches/readiness, actors, dynamic libraries,
fuzz/coverage modes and optional engines start unqualified, not implicitly enabled.
The final profile is a gate decision, not a promise that this list is sufficient.

Use target/profile capability checking to reject unavailable operations before
linking, with the operation and missing capability named. Define whether the
check applies to reachable code or all checked declarations; test generic and
indirect calls so unused embedded std declarations do not disable every program.
For capabilities genuinely determined at runtime, return an explicit existing
unsupported error where the API permits it; otherwise reject compilation.
Never provide success stubs, silently use a Linux implementation on another OS,
or drop security guarantees. If the current error API cannot express absence,
settle that API before admitting the facility.

Audit SQLite, ring/rustls, RustCrypto, compression and entropy paths separately,
including C/assembly inputs and target cfg/build-script outputs. Pure algorithms
may need only validation; native OS/assembly objects may need compatible builds
or replacement. Retain specialist crypto libraries and their constant-time and
entropy contracts. Optional dataframe/linalg engines require separately compatible
builds and independent tests; shipping a Linux engine does not make it multi-OS.
Dead stripping of uncalled modules is not a capability enforcement mechanism.

## Archive identity and link UX

Package archives with a verified manifest: compiler/runtime contract identity,
ISA and CPU baseline, C ABI/layout version, Cosmo release/hash, profile/features,
Rust toolchain and dependency lock identity, application/library mode and native
link inputs. Reject a GNU archive, wrong ISA/profile, incompatible ABI or stale
cache entry before emission/linking. Extend existing target metadata and cache
identity rather than create a second source of truth. Runtime extraction must
include the manifest and exact link argument arrays.

A qualified driver should discover or accept a pinned Cosmo toolchain, validate
its version/layout, construct profile-specific arguments and preserve paths with
spaces. The spike's missing `-lutil` justifies auditing native flags, not deleting
all GNU flags or asserting unwinding works because `-lgcc_s` resolves. Preserve
panic-abort, custom panic/OOM reporting, section retention and trap tables.
Test stripping/debug sidecars with Cosmo tools instead of assuming host `strip`
understands the final container.

Diagnostics should distinguish missing driver, incompatible archive/toolchain,
unavailable capability, linker failure and launch-route failure. Verbose output
should expose target/profile, toolchain identity and complete command arguments.
Do not hide `ENOEXEC` by treating a program as arbitrary shell text; choose a
documented loader/fallback route and preserve child arguments, status and errors.
Initially reject library exports/dynamic loading and unqualified tooling modes
explicitly, rather than emitting artifacts with untested contracts.

## Pinning, provenance and licensing

The spike used Cosmopolitan 4.0.2 (bundled GCC 14.1.0). Use that as a reproduction
baseline, not an assertion it is the best current release. Before implementation,
review upstream changes/advisories and select an exact version, archive hash and
source revision. Record compiler/runtime source, Rust version, Cargo.lock,
patches, host tools, flags and artifact hashes. Fetch only declared inputs in a
separate preparation step; exercise offline builds and reproducibility twice.

Start with an externally supplied pinned toolchain; bundling is a separate
packaging decision. The official toolchain README's Legal section distinguishes
the GCC-based toolchain from permissively licensed runtime inputs and describes
embedded notices. Audit actual shipped components and redistribution obligations,
including loaders and any compiler sources/patches; do not label the whole SDK
ISC or assume embedded notices replace Plenty's release obligations. If code or
data ships, update module headers where applicable, release third-party notices
and supply documentation. This proposal copies no upstream implementation.
