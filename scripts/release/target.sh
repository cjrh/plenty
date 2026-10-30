# shellcheck shell=bash
# Values are consumed by the scripts sourcing this file.
# shellcheck disable=SC2034
# Shared native-release target facts. Source after toolchain.env where pins are used.
# PLENTY_RELEASE_TARGET permits packaging/auditing foreign ELF without executing it;
# build.sh requires the selected target to match its native build host.
if [ -z "${PLENTY_RELEASE_TARGET:-}" ]; then
    case "$(uname -m)" in
        x86_64) PLENTY_RELEASE_TARGET=x86_64-unknown-linux-gnu ;;
        aarch64|arm64) PLENTY_RELEASE_TARGET=aarch64-unknown-linux-gnu ;;
        *) echo "unsupported native release architecture: $(uname -m)" >&2; exit 2 ;;
    esac
fi
case "$PLENTY_RELEASE_TARGET" in
    x86_64-unknown-linux-gnu)
        release_arch=x86_64
        release_machine='Advanced Micro Devices X86-64'
        release_loader=/lib64/ld-linux-x86-64.so.2
        release_cpu=x86-64
        release_cflags='-march=x86-64 -mtune=generic'
        ;;
    aarch64-unknown-linux-gnu)
        release_arch=aarch64
        release_machine=AArch64
        release_loader=/lib/ld-linux-aarch64.so.1
        release_cpu=generic
        release_cflags='-march=armv8-a -mtune=generic'
        BUILD_IMAGE="${BUILD_IMAGE_AARCH64:-}"
        SMOKE_IMAGES="${SMOKE_IMAGES_AARCH64:-}"
        RUSTUP_INIT_SHA256="${RUSTUP_INIT_SHA256_AARCH64:-}"
        MOLD_SHA256="${MOLD_SHA256_AARCH64:-}"
        ;;
    *) echo "unsupported release target: $PLENTY_RELEASE_TARGET" >&2; exit 2 ;;
esac
export PLENTY_RELEASE_TARGET
