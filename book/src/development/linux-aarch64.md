# Linux AArch64 development

The source tree implements native `aarch64-unknown-linux-gnu` alongside
`x86_64-unknown-linux-gnu`. This page describes the development harness and its
limits. It does not establish an ARM release's minimum glibc/kernel versions,
native hardware concurrency behavior, or optional-engine availability.
[Platform support](../design/toolchain/02-architecture/support.md) describes the
implemented contract; future work and admission gates remain in [B18](../backlog.md).

## Backend and C ABI probe

Install QEMU user-mode tools and Python 3, with Cargo's locked dependencies
available locally. `scripts/aarch64_probe.py` provisions a checksum-pinned Bootlin
GCC/glibc toolchain, generates ARM ELF with the pinned Cranelift backend, links a
C fixture and executes it on an emulated Cortex-A53:

```sh
ARM_TOOLS="$PWD/target/aarch64-toolchain"
python3 scripts/aarch64_probe.py --provision "$ARM_TOOLS"
cargo fetch --locked --manifest-path scripts/aarch64-probe/Cargo.toml
CARGO_TARGET_DIR="$PWD/target/aarch64-probe-build" \
    python3 scripts/aarch64_probe.py --toolchain "$ARM_TOOLS" \
    --out "$PWD/target/aarch64-probe-output"
```

Provisioning refuses to replace an existing directory. The archive is
`aarch64--glibc--stable-2025.08-1.tar.xz`, SHA-256
`dfb47eee874eef9e8a7fc042eee4e0a183f444b6bcde6a82fef8f009918389c9`.
The probe prints tool versions and ELF relocations. It exercises native C calls,
`i64` values, `u128` arguments/results, direct and indirect Tail calls
with spilled arguments, a large inline-probed frame, data addresses and a
recorded UDF trap. The expected trap runs in a child process.
Keep the tool/version output with qualification results; the toolchain's libc
is a development input, not evidence for the release's glibc 2.28 floor.

## Build the ARM compiler on x86

Cross-building the Rust compiler is a development facility. It does not add a
user-facing Plenty cross-compilation mode. Install the matching Rust target and
supply both Rust and C build tools:

```sh
rustup target add aarch64-unknown-linux-gnu
export CC_aarch64_unknown_linux_gnu="$ARM_TOOLS/bin/aarch64-buildroot-linux-gnu-gcc"
export AR_aarch64_unknown_linux_gnu="$ARM_TOOLS/bin/aarch64-buildroot-linux-gnu-ar"
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="$CC_aarch64_unknown_linux_gnu"
cargo build --locked --target aarch64-unknown-linux-gnu --bins
qemu-aarch64 -cpu cortex-a53 \
    -L "$ARM_TOOLS/aarch64-buildroot-linux-gnu/sysroot" \
    target/aarch64-unknown-linux-gnu/debug/plenty --print-target
```

Native build helpers resolve `CC_TARGET`, `CC_target_with_underscores`,
`TARGET_CC`, then `CC` (and the equivalent `AR` variables). Without an override,
cross builds choose the architecture-prefixed GNU tools, never host `cc`.
Both embedded runtime archives, native dependencies and test/fuzz helper objects
must match the selected Cargo target. Use a disk-backed `CARGO_TARGET_DIR` with
adequate space; the compiler and runtime are large enough to exhaust a small
tmpfs. Limit `CARGO_BUILD_JOBS` when sharing a machine with other builds.

A Cargo runner wraps the Rust test process only. Plenty integration tests also
spawn a compiler, linker and generated programs, so setting a QEMU runner alone
is insufficient. Use an explicit subprocess harness or run inside an ARM guest.

## Full ARM Linux guest

A full-system guest exercises an ARM kernel's signal contexts, filesystem flags
and stack behavior. The initial environment used QEMU 10.2.2, Cortex-A53 with TCG,
two virtual CPUs and 2 GiB RAM, and Debian 13 genericcloud ARM64, kernel
6.12.113 with 4 KiB pages. Its base image was downloaded from
[Debian cloud images](https://cloud.debian.org/images/cloud/trixie/latest/)
and verified against the official sibling `SHA512SUMS`:

```text
debian-13-genericcloud-arm64.qcow2
71aa5d5dbcb3d9ecfe96bc3f6dd41b6805a0a41c852204a32747d230b64b95912297fb453b11a6031fd54d84a0a0a430e137d07e6986dc9ebc9d41483be003c4
```

The `latest` URL can change: preserve the verified base image and checksum,
or obtain the same image from a dated archive. A different checksum is a new
qualification environment, not permission to bypass verification. Create a
16 GiB qcow2 overlay with `qemu-img create -f qcow2 -F qcow2 -b BASE work.qcow2
16G`. Create `meta-data` containing `instance-id: plenty-arm` and
`local-hostname: plenty-arm` on separate lines. Create this `user-data`, replacing
the key placeholder with your SSH **public** key:

```yaml
#cloud-config
users:
  - name: plenty
    groups: sudo
    shell: /bin/bash
    sudo: "ALL=(ALL) NOPASSWD:ALL"
    ssh_authorized_keys:
      - ssh-ed25519 REPLACE_WITH_YOUR_PUBLIC_KEY
package_update: true
packages: [gcc, libc6-dev, binutils, make, pkg-config, curl, git, python3]
```

Build the seed with `genisoimage -output seed.iso -volid cidata -joliet -rock
user-data meta-data`. Keep private keys and guest credentials outside the repository.

With AAVMF firmware installed, boot from the directory containing those files:

```sh
qemu-system-aarch64 -machine virt -cpu cortex-a53 -accel tcg -smp 2 -m 2048 \
    -bios /usr/share/AAVMF/AAVMF_CODE.fd \
    -drive file=work.qcow2,if=virtio,format=qcow2 \
    -drive file=seed.iso,if=virtio,format=raw,readonly=on \
    -netdev user,id=net0,hostfwd=tcp:127.0.0.1:22264-:22 \
    -device virtio-net-pci,netdev=net0 -display none \
    -serial file:console.log -monitor none -daemonize
ssh -p 22264 plenty@127.0.0.1
```

Copy the exact source revision and matching ARM binaries/test executables into
the guest, preserving executable permissions. Tests that compile C fixtures need
the guest's native `cc`, and tests that launch `plenty` need the matching compiler
at the path captured when the test executable was built. Record `uname -a`,
`getconf PAGESIZE`, `ldd --version`, `cc --version`, Rust/QEMU versions, compiler
revision and feature flags with the results. A native build inside the guest
avoids captured host paths; it needs Rust and sufficient disk/RAM as well.

## Qualification boundaries

The isolated C/backend probe has passed under QEMU user mode. The all-feature
runtime unit suite has passed in both user mode and the full-system guest above.
These results do not establish completion of the full compiler/workflow suite.
Four runtime measurements remain intentionally ignored: wide-tuple scaling,
removal timing, and two entry-operation timing/scaling benchmarks. They are not
part of the passing functional suite.

User-mode QEMU cannot establish kernel signal coexistence or ARM's real weak-memory concurrency behavior by itself. Full-system
TCG adds kernel coverage but is still emulation. Native hardware stress,
additional page sizes, release-baseline clean-container execution and optional
engine execution must each be recorded before claiming them as supported.

For the complete developer gate, run the locked workspace suite with all
features, default-feature tests, formatting, clippy and tutorial/reference
examples, as described in [development workflow](workflow.md). Run cold, warm
and cache-off compilation, C/library/Tail tests, checked traps and stack overflow,
actors, filesystem no-follow cases, crypto/compression known answers, packages,
runtime extraction, LSP and relocated distributions. Record missing services,
libFuzzer/sanitizer tools and ignored tests explicitly. Release packaging and
native runner gates are described in [releasing](releasing.md).
