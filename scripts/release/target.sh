# shellcheck shell=bash
# Values are consumed by the scripts sourcing this file.
# shellcheck disable=SC2034
# Shared native-release target facts. Source after toolchain.env where pins are used.
# PLENTY_RELEASE_TARGET permits packaging/auditing foreign ELF without executing it;
# build.sh and build-native.sh require the selected target to match the build host.
#
# release_os               linux, macos or windows
# release_exe              executable suffix
# release_archive_ext      tar.gz, or zip on Windows
# release_components       archives published for the target
# release_experimental     1 for a preview target without the Linux release contract
if [ -z "${PLENTY_RELEASE_TARGET:-}" ]; then
    case "$(uname -s)-$(uname -m)" in
        Linux-x86_64) PLENTY_RELEASE_TARGET=x86_64-unknown-linux-gnu ;;
        Linux-aarch64|Linux-arm64) PLENTY_RELEASE_TARGET=aarch64-unknown-linux-gnu ;;
        Darwin-arm64) PLENTY_RELEASE_TARGET=aarch64-apple-darwin ;;
        MINGW64*-x86_64|MSYS*-x86_64|CYGWIN*-x86_64) PLENTY_RELEASE_TARGET=x86_64-pc-windows-msvc ;;
        *) echo "unsupported native release host: $(uname -s) $(uname -m)" >&2; exit 2 ;;
    esac
fi
release_os=linux
release_exe=
release_archive_ext=tar.gz
release_components="plenty plenty-dataframe plenty-linalg"
release_experimental=0
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
    aarch64-apple-darwin)
        # Optional engines are ELF shared libraries (SONAME, $ORIGIN RUNPATH);
        # only the compiler archive ships until Mach-O engine loading exists.
        release_os=macos
        release_arch=aarch64
        release_cpu=apple-m1
        release_cflags=
        # Rust's minimum for this target; check-native.py enforces it.
        release_macos_min=11.0
        release_components=plenty
        release_experimental=1
        ;;
    x86_64-pc-windows-msvc)
        release_os=windows
        release_arch=x86_64
        release_cpu=x86-64
        release_cflags=
        release_exe=.exe
        release_archive_ext=zip
        release_components=plenty
        release_experimental=1
        ;;
    *) echo "unsupported release target: $PLENTY_RELEASE_TARGET" >&2; exit 2 ;;
esac
export PLENTY_RELEASE_TARGET
