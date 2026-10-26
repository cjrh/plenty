#!/usr/bin/env bash
# Build, audit, and package a release inside the pinned glibc 2.28 image.
#
# Usage: build.sh --out-dir DIR [--test]
#
# A relative DIR is resolved against the caller's working directory.
#
# Run inside BUILD_IMAGE from toolchain.env (the release workflow's container,
# or scripts/release/local-check.sh). Installs RUST_VERSION with a verified
# rustup-init when no matching toolchain is present, builds both executables
# with the tracked lockfile, optionally runs the full test suite with the same
# toolchain, builds the optional dataframe engine, rejects binaries above the
# glibc baseline, and packages separate compiler and engine archives.
# Set CARGO_TARGET_DIR to keep these outputs apart from host builds.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
# shellcheck source=/dev/null
. "$here/toolchain.env"

out_dir=""
run_tests=0
while [ $# -gt 0 ]; do
    case "$1" in
        --out-dir) out_dir="$2"; shift 2 ;;
        --test) run_tests=1; shift ;;
        *) echo "build.sh: unknown argument $1" >&2; exit 2 ;;
    esac
done
[ -n "$out_dir" ] || { echo "usage: build.sh --out-dir DIR [--test]" >&2; exit 2; }

glibc="$(ldd --version | sed -n '1s/.* //p')"
if [ "$glibc" != 2.28 ]; then
    echo "build.sh: running on glibc $glibc; release builds need the glibc 2.28 image ($BUILD_IMAGE)" >&2
    exit 1
fi

export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
export PATH="$CARGO_HOME/bin:$PATH"
if ! rustc +"$RUST_VERSION" --version >/dev/null 2>&1; then
    init="$(mktemp "${TMPDIR:-/tmp}/rustup-init.XXXXXX")"
    curl --proto '=https' --tlsv1.2 -sSfL -o "$init" \
        "https://static.rust-lang.org/rustup/archive/$RUSTUP_INIT_VERSION/x86_64-unknown-linux-gnu/rustup-init"
    echo "$RUSTUP_INIT_SHA256  $init" | sha256sum -c -
    chmod +x "$init"
    "$init" -y --no-modify-path --profile minimal --default-toolchain "$RUST_VERSION"
    rm -f "$init"
fi
export RUSTUP_TOOLCHAIN="$RUST_VERSION"
rustc --version
cc --version | head -n1

mkdir -p "$out_dir"
out_dir="$(cd "$out_dir" && pwd)"
cd "$repo"
target_dir="${CARGO_TARGET_DIR:-$repo/target}"
mkdir -p "$target_dir"
target_dir="$(cd "$target_dir" && pwd)"
bash "$here/install-mold.sh" "$target_dir/release-tools/mold"
export PATH="$target_dir/release-tools/mold/bin:$PATH"
export COMPILER_PATH="$target_dir/release-tools/mold/libexec/mold"
"$repo/scripts/ci_check_mold.sh"
# Generic x86-64: no host CPU tuning reaches the compiler or runtime archives.
unset RUSTFLAGS CARGO_ENCODED_RUSTFLAGS
cargo build --locked --release --bins
"$here/check-elf.sh" "$target_dir/release/plenty" "$target_dir/release/plentyc"
if [ "$run_tests" = 1 ]; then
    # Keep the tests' compiled-function cache with this build's outputs.
    PLENTY_CACHE_DIR="$target_dir/plenty-cache" cargo test --locked --workspace --all-features
fi
"$here/package.sh" --bin-dir "$target_dir/release" --out-dir "$out_dir" \
    --rust-sysroot "$(rustc --print sysroot)"

# Keep the engine optional in normal compiler builds and in its own download.
# This uses the same pinned toolchain, libc and source as the compiler archive.
python3 -B scripts/build_dataframe_engine.py --out "$target_dir/dataframe-install" \
    --target-dir "$target_dir/dataframe-engine" --jobs "${CARGO_BUILD_JOBS:-3}"
python3 -B "$here/package-dataframe.py" \
    --engine "$target_dir/dataframe-install/libplenty_dataframe_v1.so" \
    --out-dir "$out_dir" --rust-sysroot "$(rustc --print sysroot)"
