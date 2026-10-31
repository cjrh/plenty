# Native ABI and Zig toolchain experiments

> Evidence from 2026-10-31. These are small toolchain probes, not Plenty port
> qualification. Decisions and implementation sequence belong to
> [B18](../../backlog.md); [interface boundaries](boundaries.md) describe the design.

## Native Cranelift CI smoke tests

The standalone [platform-probes repository](https://github.com/plentylang/platform-probes)
now runs push-triggered native jobs on macOS ARM64, Windows x86-64 MSVC and
Linux x86-64. At probe commit `ded48e7d6570dba951a954d0de09e1e93749fc14`,
[all three jobs passed](https://github.com/plentylang/platform-probes/actions/runs/41163832486).
The fixture uses Rust 1.99.1 and Cranelift 0.137.0; each job checks its host
triple, emits a native object, links a C harness using the platform compiler,
and executes seven assertions. Objects, executables and logs are CI artifacts.

The assertions cover ten integer arguments, mixed integer/f64 arguments,
signed/unsigned narrow arguments, generated-to-C calls for those signatures,
and a native entry calling an internal `Tail` function. Native CI confirms
these paths execute on Apple AArch64 and WindowsFastcall, rather than merely
producing a Mach-O or PE file. This fixture does not compile Plenty itself.

The initial run exposed two harness configuration errors. Enabling Cranelift
`is_pic=true` removed Darwin's rejected absolute text relocations; Plenty
already enables that setting. Replacing filesystem canonicalization with
ordinary absolute output paths avoided MSVC driver mishandling of Windows
verbatim path prefixes. Both corrections were verified by the successful run.

This is partial B18.1 evidence. Indirect/tail-return calls, large frames and
stack probes, u128/aggregates, Rust ABI boundaries, unwind/exception handling,
minimum OS versions and full runtime/dependency builds remain unqualified.
Windows GNU is not covered by this native suite; MSVC is the working probe
baseline, not yet a qualified Plenty release contract.

The Plenty repository's `portability.yml` workflow additionally builds the actual
workspace on these macOS and Windows runners and runs unit tests if compilation
succeeds. It preserves build logs and reports failures while the ports are
incomplete. Existing `ci.yml` jobs exercise Linux x86-64 and ARM64, including
optional engines and Docker proxy tests. A successful standalone probe is not
substituted for any of these compiler/runtime checks.

## Two independent uses

`cargo-zigbuild` can help build the Rust compiler, its runtime and C dependencies.
Separately, Plenty could invoke `zig cc` as the driver linking Cranelift-produced
objects with a matching runtime. The latter needs Plenty's own target-aware link
plan and artifact selection; cargo-zigbuild does not add that user feature.
Cranelift remains responsible for Plenty machine-code generation.

The important Zig mechanism is more than a linker executable: it carries target
information, libc headers/sources or interface stubs, compiler support routines,
and logic for producing target CRT/import artifacts. These inputs still have
coverage and version limits. Full system SDKs and external libraries may remain
necessary. Avoid promising arbitrary host-to-target builds based on a simple
program, or confusing producing an executable with testing it on that OS.

## Source and tool identities

| Input | Inspected identity |
| --- | --- |
| Plenty | `869e1cb8087c126ec26a620149bd74d2290c4ca7` |
| Local Zig source | `4dd65e63f23569782f1560d40217cc74aae8b6cb` (development source) |
| Executed Zig | Installed `/bin/zig`, version 0.16.0; distinct from the source checkout |
| Executed cargo-zigbuild | 0.24.0; upstream tag resolves to `2ffd80658c96b5ff971085dd509e58e5b441f1c8` |
| Executed Rust | 1.99.1 |

The Zig checkout's `src/Compilation.zig` loads bundled Darwin `libSystem.tbd`
and `SDKSettings.json` from `lib/libc/darwin`, and builds MinGW CRT/import
libraries. Its `src/main.zig` has native MSVC library discovery. These are
specific toolchain facilities, not evidence for portable Plenty runtime behavior.

The [cargo-zigbuild documentation](https://github.com/rust-cross/cargo-zigbuild/tree/v0.24.0)
describes target-qualified compiler wrappers, glibc-version selection,
`SDKROOT`, deployment targets and limitations. Its integration must reach all
of Plenty's direct C/archive commands, not only Cargo's final Rust linker.
Pin the tool version and inspect produced imports rather than assuming the
requested libc baseline is sufficient proof.

## Executed small probes

An isolated dependency-free Cargo package in `/tmp/plenty-zig-port-probe`
compiles `probe.c` using `CC_<target_with_underscores>`, archives it using
`AR_<target_with_underscores>`, and links it into a Rust executable. Its C
function calls `puts` and returns its integer argument plus nine; Rust calls it
with two and asserts eleven. The helper invokes each tool as an executable
path, matching a relevant constraint of Plenty's direct native build.

| Experiment | Observed result | What it establishes |
| --- | --- | --- |
| Rust+C `aarch64-unknown-linux-gnu.2.28` | Build succeeded; ELF imported glibc symbols no newer than 2.28; QEMU Cortex-A53 execution exited zero and printed both messages | Working wrapper/archiver handoff and small C ABI call; execution used glibc 2.41, so minimum-libc execution is unproved |
| Rust+C `aarch64-apple-darwin`, deployment 13.0, no `SDKROOT` | Build succeeded; ARM64 Mach-O reports minimum 13.0, SDK 26.4 and libSystem dependency | Zig bundled stubs suffice for this tiny artifact; no macOS execution occurred |
| Darwin CoreFoundation header | Compilation failed without full SDK headers | Bundled libSystem stubs are not a complete Apple SDK |
| Windows MSVC C using `stdio.h` | Compilation failed without SDK/CRT headers | Local environment does not supply the required MSVC development inputs |
| Standalone Windows GNU C | Produced x86-64 PE32+ with UCRT API DLL imports | Small MinGW artifact construction only; no Windows/Wine execution, and no Rust GNU target installed for a Rust+C probe |

Logs, sources and artifacts remain in that task-owned temporary directory.
`linux-run.json` records the exact QEMU command and exit status. The successful
builds do **not** compile Plenty's embedded runtime, ring/SQLite/compression,
optional engines, generated `Tail` calls or exception recovery. Those are
explicit subsequent experiments, not inferred successes.

### Reproduction inputs

The original fixture and exact environment are recorded in
`/tmp/plenty-zig-port-probe/REPORT.md`. The executed build commands, from that
fixture directory, were:

```sh
export CARGO_ZIGBUILD_CACHE_DIR="$PWD/wrappers"
export ZIG_GLOBAL_CACHE_DIR="$PWD/zig-cache"
export CARGO_TARGET_DIR="$PWD/target"
export CARGO_BUILD_JOBS=1
cargo zigbuild --offline --target aarch64-unknown-linux-gnu.2.28 -v
MACOSX_DEPLOYMENT_TARGET=13.0 cargo zigbuild --offline --target aarch64-apple-darwin -v
cargo zigbuild --offline --target x86_64-pc-windows-msvc -v
zig cc -target aarch64-macos -c framework.c -o framework.o
zig cc -target x86_64-windows-gnu windows.c -o windows-gnu.exe
```

The first two commands succeeded; the MSVC and framework commands failed as
recorded above. `framework.c` includes `CoreFoundation/CoreFoundation.h` and
references `CFGetTypeID(kCFBooleanTrue)`; `windows.c` prints one message from
`main` using `stdio.h`. All dependencies are system/toolchain inputs; no crates
were downloaded for the fixture. The QEMU command was:

```sh
qemu-aarch64 -cpu cortex-a53 \
  -L /tmp/plenty-arm-toolchain/aarch64-buildroot-linux-gnu/sysroot \
  target/aarch64-unknown-linux-gnu/debug/plenty-zig-port-probe
```

Recorded SHA-256 artifact identities (build paths affect reproducibility):

| Artifact | SHA-256 |
| --- | --- |
| Linux Rust+C ELF | `9e248f61e6cedff324c9e0c7e25eb757432d8348bce96f5b8bd0a19d6ded360f` |
| Darwin Rust+C Mach-O | `78784e5bc56a76a2f98552b486706120a6121816a72ecfe7e5eb27bc0a7804d1` |
| Windows GNU C PE | `46b4e2498719abb500ba3f6a35b114e1b827882eb2bc04a665f1de2c3e32948c` |

## Admission experiments

Run these in isolated probe packages before expanding Plenty's target allowlist:

- Use pinned Cranelift to emit native wrappers around direct/indirect `Tail`
  calls, spilled arguments and large frames for each candidate triple. Link and
  execute on CI; inspect actual relocations, stack probes and unwind records.
- Build both actual embedded runtime variants and their C/assembly dependencies
  with the proposed toolchain. Compare Cargo and direct-build configurations,
  panic-abort aliases, archive member architectures, imports and notices.
- Round-trip signed/unsigned narrow integers, u128, floats, stack arguments and
  callbacks through Rust/C/Plenty. Include separate Windows GNU and MSVC fixtures
  before selecting one distribution contract.
- Exercise interface/source/trap/coverage metadata through static/shared links,
  dead stripping, relocated installation and library hosts. Verify resident
  code lifetime after loader leases close.
- Build and execute dataframe/linalg using the supported packaging helper.
  Audit platform library identities/search paths and allocator ownership.
- Repeat using pinned full Apple SDK/Windows SDK inputs and minimum supported
  systems. Audit imports and execute offline from extracted packages without
  Rust/Cargo. Cross-linking remains supplementary to native CI execution.

## Decisions informed by the probes

Keep native macOS/Windows runners as the correctness baseline. Choose the
Windows ABI after actual dependency and runtime probes; native MSVC is a useful
first candidate, while MinGW is a separate CRT/link/distribution choice. A
successful GNU C executable does not establish MSVC compatibility.

For macOS, distinguish a libSystem-only artifact from SDK/framework-dependent
builds and record deployment target explicitly. For Linux, retain the existing
glibc floor unless evidence warrants changing it; the user permits raising the
floor to 2.30, but these probes do not require that change.

Do not bundle Zig, SDKs or libraries into Plenty releases without deciding
installation size, redistribution/license obligations, offline behavior and
version pinning. First prove whether invoking an installed driver meets the
native-port goal. User-facing cross-compilation can later reuse the boundaries
without forcing a toolchain-bundling decision into this port.
