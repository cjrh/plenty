# Cosmo evidence and qualification gates

> Part of the [Cosmopolitan proposal](../cosmopolitan.md). Spike observations are
> not implementation or support claims. Gates define required evidence, not a
> parallel priority list; scheduling remains B18/B17 in the [backlog](../../backlog.md).

## Retained experiment and provenance

The completed spike inspected main `2851ef3481f70dec580eb76d2651b86524d16dfb` in
`/tmp/plenty-cosmopolitan-20260718`. Its report is retained locally at
`/tmp/plenty-cosmopolitan-spike-report.md`, with inputs, logs and binaries in
`/tmp/plenty-cosmopolitan-20260718-artifacts`. These are local evidence locations,
not required book assets or reproducible public downloads. The investigation
worktree remains unchanged. The observations below preserve the essential
results even when those scratch paths are unavailable.

The experiment **used a retained prebuilt Plenty compiler and its embedded
runtime**, not a fresh build from the inspected source. It reported Plenty
0.7.1, `x86_64-unknown-linux-gnu`; retained binary SHA-256:
`489623df196e84fd201417dae5a389574af184b4c7e1dc78f8a74c05e186d48f`.
The downloaded Cosmopolitan 4.0.2 archive SHA-256 was
`85b8c37a406d862e656ad4ec14be9f6ce474c1b436b9615e91a55208aced3f44`.
Source inspection and binary execution therefore have different provenance.

Only **Linux x86-64** was tested. No Windows, macOS, BSD, AArch64, BIOS,
networking, worker-thread, native engine or unwind qualification was performed.
No compiler/runtime source was changed by the spike.

| Probe | Retained evidence | Observed result |
| --- | --- | --- |
| C control with ordinary `cosmocc` | `c-control-build.log`, `c-control-run.log` | Shell launch prints control message; not a Plenty runtime test |
| Native Plenty empty and print/list allocation | `empty.native-*.log`, `print-alloc.native-*.log` | Build/run succeed |
| Existing Plenty linker override | `print-alloc.cosmo-build.log`, `empty.cosmo-build.log` | Link fails: cannot find `-lutil` |
| Exact emitted object + extracted runtime, omit only `-lutil` | `*.filtered-command.txt`, `*.filtered-build.log`, `empty.filtered-run.log`, `print-alloc.filtered-run.log` | Empty and print/list allocation link and run; output `cosmo Plenty [1, 2, 3]` |
| Division by zero | `trap.native.run.log`, `trap.filtered.com.run.log` | Native reports source-site error and exits 1; Cosmo exits -4 (`SIGILL`) with no diagnostic |
| C sigaction layout measurement | `abi.native.log`, `abi.cosmo.log`, `abi-control.c` | GNU size/flags/mask/restorer offsets 152/136/8/144; Cosmo 32/8/24/16 |
| Direct subprocess execution | `direct-exec.log` | C control and both successful Plenty APEs return `ENOEXEC`; shell fallback succeeds |

The ELF64 x86-64 objects linked with their measured PC-relative relocations.
This establishes basic object/link compatibility for these inputs, not complete
runtime compatibility. The signal-layout mismatch is measured; that it explains
the trap failure is a strong hypothesis, not an isolated causal result from a
patched runtime. Successful startup does not prove handlers were installed
correctly. TLS fixup/source inspection and observed GS accesses leave TLS
compatibility unresolved rather than demonstrating either universal safety or
universal failure.

`reproduce.sh` retains consolidated commands, but was not rerun as a suite and
ends with an expected failing as-is link. It is not a passing regression test.
Do not rerun expensive probes merely to write this proposal; the first approved
implementation experiment must replace the prebuilt provenance gap.

## Staged go/no-go criteria

Each stage requires review of the evidence before expanding scope. Stop with a
recorded limitation if acceptance fails; do not expand an allowlist to bypass it.

| Gate | Bounded experiment / acceptance criteria | Decision enabled |
| --- | --- | --- |
| Reproducible baseline | Fresh locked compiler and runtime build at a recorded commit; exact Rust/Cosmo inputs and commands; repeat native and Cosmo empty/allocation/trap results; retain source, hashes, logs and extracted manifest | Confirm the spike applies to reproducible source, or investigate the discrepancy before proceeding |
| x86-64 runtime compatibility | Cosmo-header signal/context shim; checked traps with site/status, unknown-signal forwarding, stack overflow on main and worker threads, page probing, allocation failure/panic/OOM reporting, repeated thread start/join/failure, isolated TLS/destructors and contended synchronization | Decide whether a bounded GNU-component compatibility model is defensible; otherwise stop or approve a dedicated runtime-build investigation |
| Real multi-OS execution | Transfer the exact same hashed x86-64 artifact to at least one real non-Linux OS; run core success/failure/thread tests through a documented loader route, without rebuilding it per OS | Decide whether to adopt a restricted executable profile; Linux-only success is insufficient |
| Restricted preview integration | Reviewed capability set, explicit negative tests, target/archive/cache validation, pinned driver and reproducible offline packaging; preserve native GNU tests | Admit only named profile/OS/version/CPU configurations as experimental, with limitations |
| Broader support qualification | Module/dependency and OS security semantics pass independently; extracted-release tests on every claimed OS/ISA/version baseline, deployment policy and notices complete | Support announcement only for the qualified matrix; no automatic admission of upstream's entire list |
| Optional fat output | Independent AArch64 Cosmo ABI/runtime qualification, matching features and contracts, `apelink` packaging, identical combined artifact exercised on real x86-64 and AArch64 systems | Decide whether fat output merits its maintenance cost; not a prerequisite for x86-64 multi-OS support |

The runtime compatibility gate must not disable trap reporting or stack
protection to obtain a green result. If signal-context rewriting cannot be made
safe, compare helper-call checks explicitly while retaining proper stack fault
handling; document the performance/semantic consequences before proceeding.
No requirement here implies stack-overflow recovery: preserve the current
executable diagnostic and abort behavior, and separately qualify any admitted
actor trap containment.

## Testing and CI design

Keep native GNU/Linux regression jobs unchanged. Add a separately named,
opt-in experiment lane before release jobs: build the compiler fresh, record
provenance, compile a behavioral corpus once, and publish hashes and artifacts
for real-OS jobs. Non-Linux jobs consume those artifacts, not recompile convenient
OS-specific replacements. Cross builds and emulators supplement ABI debugging;
they cannot establish real signal, loader, filesystem-policy or scheduling behavior.

Exercise debug/release, cache on/off, paths with spaces, failure injection,
large/recursive frames, integer and collection traps, foreign unknown faults,
handler coexistence, thread startup failure and repeated cleanup. Test allocation
balance and per-thread state after jobs. Check profile rejection via direct,
indirect and generic calls, unavailable optional engines, wrong runtime manifests,
stale caches, missing tools and direct-exec/loader errors. Add full compiler and
runtime all-feature suites and executable documentation tests for admitted
features; explicitly label exclusions instead of broadly skipping non-Linux
failures. Library mode requires separate host-coexistence and ABI tests if admitted.

For filesystem/network expansion, include adversarial rooted paths, native names,
resource leaks, cancellation/readiness, DNS, TLS entropy and handshake errors,
process cleanup and unavailable facilities. Reuse the contracts in the
[native OS semantics assessment](../cross-platform/stdlib.md), not guessed Linux
errno mappings. Check absence of accidental dynamic GNU/Linux dependencies and
unqualified syscall paths in the final reachable runtime.

Release qualification additionally needs a clean extracted-package test with
only documented tools, no Rust/Cargo or network for end users, correct status
propagation, loader extraction failure/noexec cases, and OS download/launch policy
including signing where required. Record which machines, versions, CPUs and
routes actually ran. CI configuration alone does not qualify a platform; real
runners and retained passing results are a go/no-go dependency.

## Official sources and source-credit boundary

The spike checked official sources over HTTPS and retained copies. These links
are versioned where possible; no additional external platform claim is needed
for this proposal:

- [Cosmopolitan 4.0.2 release](https://github.com/jart/cosmopolitan/releases/tag/4.0.2): downloaded toolchain baseline and upstream scope.
- [Toolchain README at 4.0.2](https://github.com/jart/cosmopolitan/blob/4.0.2/tool/cosmocc/README.md): per-ISA drivers, `apelink`, loader paths, installation/gotchas and Legal section. Retained bundle copy: `cosmocc/README.md`.
- [Inspected current toolchain README snapshot](https://github.com/jart/cosmopolitan/blob/aefad27c09ffda64c6ac64bd69f758e9f9c7b91d/tool/cosmocc/README.md): context only, not a moving version pin; retained as `official-current-toolchain.md`.
- [sigaction definition](https://github.com/jart/cosmopolitan/blob/4.0.2/libc/calls/struct/sigaction.h): checked against the C layout probe; retained as `official-sigaction.h`.
- [TLS header](https://github.com/jart/cosmopolitan/blob/4.0.2/libc/thread/tls.h) and [object fixups](https://github.com/jart/cosmopolitan/blob/4.0.2/tool/build/fixupobj.c): TIB and eligible TLS instruction adaptation; fixup source retained as `official-fixupobj.c`.
- [APE source](https://github.com/jart/cosmopolitan/blob/4.0.2/ape/ape.S): loader/container mechanics; retained as `official-ape.S`.

The spike's `ape-readme.md` is a 404, not supporting evidence. No upstream code,
data, toolchain or executable is added to the repository by this documentation
proposal; implementation-stage redistribution requires the audit described in
[pinning and licensing](runtime.md#pinning-provenance-and-licensing).
