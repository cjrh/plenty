#!/usr/bin/env bash
# Build, audit, and package an experimental macOS or Windows release natively.
#
# Usage: build-native.sh --out-dir DIR [--test] [--phase all|prepare|package]
#                        [--engine-dir DIR]
#
# Run with bash on a macos-15 (aarch64-apple-darwin) or windows-2025
# (x86_64-pc-windows-msvc, Git Bash) runner with RUST_VERSION from
# toolchain.env installed through rustup, with rustfmt and clippy for native
# engine validation. The host must match the target: there
# is no cross build. --test runs the checks that native CI qualifies on these
# platforms (portability.yml): platform-boundary tests and the compiler and
# runtime unit tests, not the full integration suite. Only the core `plenty`
# archive includes the dataframe dylib on macOS or DLL on Windows.
# CI prepares the compiler and engines in parallel. The prepare phase builds
# and optionally tests the compiler without packaging; package consumes those
# binaries plus previously validated engines from --engine-dir. The default all
# phase still builds and validates everything locally. Packaging always audits
# the supplied binaries and engines.
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
phase=all
engine_dir=""
while [ $# -gt 0 ]; do
    case "$1" in
        --out-dir) out_dir="$2"; shift 2 ;;
        --test) run_tests=1; shift ;;
        --phase) phase="$2"; shift 2 ;;
        --engine-dir) engine_dir="$2"; shift 2 ;;
        *) echo "build-native.sh: unknown argument $1" >&2; exit 2 ;;
    esac
done
[ -n "$out_dir" ] || { echo "usage: build-native.sh --out-dir DIR [--test]" >&2; exit 2; }
case "$phase" in
    all|prepare) [ -z "$engine_dir" ] || { echo "--engine-dir requires --phase package" >&2; exit 2; } ;;
    package) [ -n "$engine_dir" ] && [ "$run_tests" = 0 ] || { echo "--phase package requires --engine-dir and rejects --test" >&2; exit 2; } ;;
    *) echo "unknown native build phase: $phase" >&2; exit 2 ;;
esac
# Resolve before changing directory, including relative caller paths.
if [ -n "$engine_dir" ]; then engine_dir="$(cd "$engine_dir" && pwd)"; fi
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
if [ "$phase" != package ]; then
    cargo build --locked --release --bins
    if [ "$run_tests" = 1 ]; then
        cargo test --locked --manifest-path plenty-runtime/platform-tests/Cargo.toml
        PLENTY_CACHE_DIR="$target_dir/plenty-cache" \
            cargo test --locked --workspace --lib --no-fail-fast -- --test-threads=1
    fi
fi
if [ "$phase" = prepare ]; then exit 0; fi
engines=()
for name in $release_engines; do
    if [ "$phase" = all ]; then
        "${PYTHON:-python3}" -B scripts/build_native_engine.py "$name" --out "$target_dir/release" --validate
        engine_dir="$target_dir/release"
    fi
    if [ "$release_os" = macos ]; then library="libplenty_${name}_v1.dylib"; else library="plenty_${name}_v1.dll"; fi
    engines+=(--engine "$name=$engine_dir/$library")
done
"$here/package.sh" "${engines[@]}" --bin-dir "$target_dir/release" --out-dir "$out_dir" \
    --rust-sysroot "$(rustc --print sysroot)"
