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
# shellcheck source=target.sh
. "$here/target.sh"

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
        "https://static.rust-lang.org/rustup/archive/$RUSTUP_INIT_VERSION/$PLENTY_RELEASE_TARGET/rustup-init"
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
if [ "$release_arch" = aarch64 ]; then
    # Upstream mold 3.0.0's ARM binary imports gettid@GLIBC_2.30 and
    # cannot run in this glibc 2.28 image. Use the image's pinned GNU linker;
    # Ubuntu ARM host CI can still use the separately pinned mold binary.
    release_ld="$(cc -print-prog-name=ld)"
    release_ld="$(command -v "$release_ld")"
    "$release_ld" --version | sed -n '1p'
    COMPILER_PATH="$(dirname "$release_ld")"
    export COMPILER_PATH
else
    bash "$here/install-mold.sh" "$target_dir/release-tools/mold"
    export PATH="$target_dir/release-tools/mold/bin:$PATH"
    export COMPILER_PATH="$target_dir/release-tools/mold/libexec/mold"
    "$repo/scripts/ci_check_mold.sh"
fi
# Baseline CPU: no host tuning reaches the compiler or runtime archives.
unset RUSTFLAGS CARGO_ENCODED_RUSTFLAGS
export RUSTFLAGS="-C target-cpu=$release_cpu"
export PLENTY_RUNTIME_RUSTFLAGS="-C target-cpu=$release_cpu"
export CFLAGS="$release_cflags" CXXFLAGS="$release_cflags"
host="$(rustc -vV | sed -n 's/^host: //p')"
[ "$host" = "$PLENTY_RELEASE_TARGET" ] || { echo "build.sh: target $PLENTY_RELEASE_TARGET differs from host $host" >&2; exit 1; }
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
engine_validation=()
if [ "$run_tests" = 1 ]; then
    rustup component add rustfmt clippy
    engine_validation+=(--validate)
fi
for engine in dataframe linalg; do
    python3 -B scripts/build_native_engine.py "$engine" --out "$target_dir/$engine-install" \
        --target-dir "$target_dir/$engine-engine" --jobs "${CARGO_BUILD_JOBS:-3}" "${engine_validation[@]}"
    python3 -B "$here/package-engine.py" "$engine" \
        --engine "$target_dir/$engine-install/libplenty_${engine}_v1.so" \
        --out-dir "$out_dir" --rust-sysroot "$(rustc --print sysroot)"
done
