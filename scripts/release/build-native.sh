#!/usr/bin/env bash
# Build, audit, and package an experimental macOS or Windows release natively.
#
# Usage: build-native.sh --out-dir DIR [--test]
#
# Run with bash on a macos-15 (aarch64-apple-darwin) or windows-2025
# (x86_64-pc-windows-msvc, Git Bash) runner with RUST_VERSION from
# toolchain.env installed through rustup. The host must match the target: there
# is no cross build. --test runs the checks that native CI qualifies on these
# platforms (portability.yml): platform-boundary tests and the compiler and
# runtime unit tests, not the full integration suite. Only the core `plenty`
# archive is packaged; the optional engines are Linux ELF libraries.
# PYTHON selects the interpreter (default python3; Windows runners use python).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
# shellcheck source=/dev/null
. "$here/toolchain.env"
# shellcheck source=target.sh
. "$here/target.sh"

out_dir=""
run_tests=0
while [ $# -gt 0 ]; do
    case "$1" in
        --out-dir) out_dir="$2"; shift 2 ;;
        --test) run_tests=1; shift ;;
        *) echo "build-native.sh: unknown argument $1" >&2; exit 2 ;;
    esac
done
[ -n "$out_dir" ] || { echo "usage: build-native.sh --out-dir DIR [--test]" >&2; exit 2; }
if [ "$release_os" = linux ]; then
    echo "build-native.sh: $PLENTY_RELEASE_TARGET builds in the glibc image with build.sh" >&2
    exit 2
fi

export RUSTUP_TOOLCHAIN="$RUST_VERSION"
rustc --version
host="$(rustc -vV | sed -n 's/^host: //p')"
[ "$host" = "$PLENTY_RELEASE_TARGET" ] || { echo "build-native.sh: target $PLENTY_RELEASE_TARGET differs from host $host" >&2; exit 1; }

mkdir -p "$out_dir"
out_dir="$(cd "$out_dir" && pwd)"
cd "$repo"
target_dir="${CARGO_TARGET_DIR:-$repo/target}"
mkdir -p "$target_dir"
target_dir="$(cd "$target_dir" && pwd)"

# Baseline CPU: no host tuning reaches the compiler or runtime archives.
unset RUSTFLAGS CARGO_ENCODED_RUSTFLAGS
export RUSTFLAGS="-C target-cpu=$release_cpu"
export PLENTY_RUNTIME_RUSTFLAGS="-C target-cpu=$release_cpu"
if [ "$release_os" = macos ]; then
    # Both rustc and the cc crate (runtime C sources) honour this; check-native.py
    # rejects an executable whose LC_BUILD_VERSION asks for a newer macOS.
    export MACOSX_DEPLOYMENT_TARGET="$release_macos_min"
    cc --version | head -n1
fi
cargo build --locked --release --bins
if [ "$run_tests" = 1 ]; then
    cargo test --locked --manifest-path plenty-runtime/platform-tests/Cargo.toml
    PLENTY_CACHE_DIR="$target_dir/plenty-cache" \
        cargo test --locked --workspace --lib --no-fail-fast -- --test-threads=1
fi
"$here/package.sh" --bin-dir "$target_dir/release" --out-dir "$out_dir" \
    --rust-sysroot "$(rustc --print sysroot)"
