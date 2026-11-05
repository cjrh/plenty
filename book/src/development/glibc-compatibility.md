# Linux release glibc verification

Issue #730 asked whether the Linux release baseline was glibc 2.28 or 2.30.
The **v0.8.9 published archives support glibc 2.28** on both x86-64 and
AArch64. The release text's 2.28 requirement needs no correction. This audit
was performed on 2026-11-05; it does not qualify unpublished builds.

## Published artifact evidence

The downloaded v0.8.9 archives matched their published SHA-256 files:

| Archive | SHA-256 |
|---|---|
| `plenty-0.8.9-x86_64-unknown-linux-gnu.tar.gz` | `249526b6536b503227e9b9e176398e74a0becddb8eb3db14a3d6e3b53134015a` |
| `plenty-0.8.9-aarch64-unknown-linux-gnu.tar.gz` | `370bb7541ab3ebd2026ca0cbeb036814231bf32728296634c5f9153812c4c2da` |

For each architecture, `scripts/release/check-elf.sh` passed for `bin/plenty`,
`bin/plentyc`, `bin/libplenty_dataframe_v1.so` and
`bin/libplenty_linalg_v1.so`. All eight files have a highest referenced GLIBC
version of **2.28** and GCC symbol version of **4.2.0**. The 2.28 reference is
`statx`, a weak undefined symbol. No file references `GLIBC_2.30` or a later
version. The audit also checked ELF architecture, loader, allowed dynamic
libraries, absence of RPATH/RUNPATH and engine SONAMEs. Symbol inspection alone
is not an execution test or proof that earlier glibc versions are supported.

## Baseline execution

The published archives were tested with the existing
`scripts/release/smoke-in-container.sh` harness, using the pinned AlmaLinux 8
images from `scripts/release/toolchain.env`:

| Architecture | Image manifest digest | Execution |
|---|---|---|
| x86-64 | `sha256:8b469a3a78515e8a18ea8fc727e6a3679e1d0c5ba6f58d5b30be3d0d9b86cffe` | Native x86-64 container |
| AArch64 | `sha256:29fdedeed8a6707558c9e03b5d3e8f09eac5307cdd0811218d7d55a2cc9ae522` | ARM64 container under QEMU user emulation |

Both containers reported AlmaLinux 8.10 and glibc 2.28. The harness installed
only the documented `gcc tar gzip` prerequisites, disconnected the network,
and ran the extracted archives without Rust or a source checkout. It checked
both compiler version commands, compiled and executed a program using SQLite
and SHA-256, ran a generated project, and repeated the checks after relocating
the installation and installing a second copy. The dataframe and linalg
smokes additionally exercised their native engines, optional linkage and
app-only `--dist` relocation. All three unmodified smoke scripts passed on
x86-64.

The stock ARM compiler smoke first passed compilation, execution and the
project check, then failed its missing-linker diagnostic assertion. Under
QEMU 10.2.2, invoking absent `cc` reported
``linker driver `cc` failed (exit status: 127)`` rather than the expected
installation hint. A temporary copy of `smoke-test.sh` accepted either diagnostic for that
one negative test; the nonzero-exit requirement and all other checks stayed
unchanged. The repeated compiler smoke and both unmodified engine smokes
passed. This adaptation is not a change to the release harness or evidence
that the original ARM diagnostic assertion passed.

These are libc/userspace checks on host Linux `7.2.11-200.fc44.x86_64`. They do not qualify the
oldest supported kernel, native ARM concurrency or other ARM page sizes.
They establish the supported 2.28 baseline for these artifacts; they do not
claim that every older version fails or every library operation was exercised.

## Current source and generated programs

The source inspected was main commit
`fefe4bae` (the audit worktree base); the published tag was v0.8.9, locally
resolved to `f65fdda439f8af2e15289c441f5aa626ab59c808`.
Current `build.sh` requires glibc 2.28, both release build images are pinned
`manylinux_2_28` images, and `check-elf.sh` rejects release binaries above
2.28. This audit did not rebuild current main and does not substitute for its
release qualification. The Linux build, ELF audit and smoke scripts used here
are unchanged between that tag and the audit base.

The 2.30 reference in the [release workflow explanation](releasing.md#what-the-workflow-does)
concerns the upstream **ARM mold 3.0.0 build tool**: its binary imports
`gettid@GLIBC_2.30`. The ARM baseline build uses its image's GNU linker instead.
Mold is not shipped in the Plenty archive and is not an installation dependency.
This tool requirement does not raise the compiler's glibc baseline.

Programs link against the compiling machine's C library and startup objects.
A program built on a newer distribution may require a newer glibc even when
its compiler runs on 2.28. Build on the oldest supported deployment system,
and audit and execute the resulting program there. Programs using dataframe
or linalg also need the matching engine; the shipped engines passed this
2.28 audit. Locally rebuilt or substituted engines have their own requirements.
See [installation requirements](../design/toolchain/01-building/installing.md#requirements)
and [native linking](../design/toolchain/01-building/linking.md).

## Repeating the check

Download both archives and their checksum files for the release being checked.
From the download directory, verify each checksum, extract the archive, then
change to the source checkout and run its audit for each target. Set `target`
to `x86_64-unknown-linux-gnu` or `aarch64-unknown-linux-gnu`, and `root` to the
absolute path of the extracted archive directory:

```sh
export PLENTY_RELEASE_TARGET="$target"
bash scripts/release/check-elf.sh "$root/bin/plenty" "$root/bin/plentyc"
for engine in dataframe linalg; do
    library="libplenty_${engine}_v1.so"
    bash scripts/release/check-elf.sh --engine "$library" "$root/bin/$library"
done
```

Run `smoke-in-container.sh IMAGE ARCHIVE` with each pinned baseline image and
its matching archive. On an x86-64 host with ARM binfmt/QEMU already configured,
set `DOCKER_DEFAULT_PLATFORM=linux/arm64` for the ARM invocation. Keep the
checksum file beside the archive. Record the release identity, hashes,
image digests, audit output and smoke exit statuses; do not infer artifact
compatibility solely from the source configuration.
