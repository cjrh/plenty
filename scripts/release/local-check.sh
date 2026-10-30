#!/usr/bin/env bash
# Run the release build and archive checks locally with Docker, as the
# release workflow does, without tagging, uploading, or publishing anything.
#
# Usage: local-check.sh [--test] [--skip-smoke]
#
# Builds in BUILD_IMAGE into target/release-container/ (separate from host
# builds), then smoke-tests both archives in every SMOKE_IMAGES container.
# --test also runs the full test suite in the build container.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
# shellcheck source=/dev/null
. "$here/toolchain.env"
# shellcheck source=target.sh
. "$here/target.sh"

[ "$(uname -m)" = "$release_arch" ] || {
    echo "local-check.sh: $PLENTY_RELEASE_TARGET requires a native $release_arch host" >&2
    exit 1
}

build_args=()
smoke=1
for option in "$@"; do
    case "$option" in
        --test) build_args+=(--test) ;;
        --skip-smoke) smoke=0 ;;
        *) echo "local-check.sh: unknown option $option" >&2; exit 2 ;;
    esac
done

out="$repo/target/release-container-$release_arch"
mkdir -p "$out/home" "$out/tmp"
rm -rf "$out/dist"
docker run --rm --label org.plenty.release-build=1 \
    --user "$(id -u):$(id -g)" \
    -v "$repo:/work" -w /work \
    -e HOME="/work/target/release-container-$release_arch/home" \
    -e CARGO_TARGET_DIR="/work/target/release-container-$release_arch/target" \
    -e TMPDIR="/work/target/release-container-$release_arch/tmp" \
    -e SOURCE_DATE_EPOCH="$(git -C "$repo" log -1 --format=%ct)" \
    ${CARGO_BUILD_JOBS:+-e CARGO_BUILD_JOBS="$CARGO_BUILD_JOBS"} \
    ${RUST_TEST_THREADS:+-e RUST_TEST_THREADS="$RUST_TEST_THREADS"} \
    "$BUILD_IMAGE" \
    scripts/release/build.sh --out-dir "/work/target/release-container-$release_arch/dist" "${build_args[@]}"

archive="$(ls "$out"/dist/plenty-[0-9]*.tar.gz)"
engine="$(ls "$out"/dist/plenty-dataframe-*.tar.gz)"
linalg="$(ls "$out"/dist/plenty-linalg-*.tar.gz)"
if [ "$smoke" = 1 ]; then
    for image in $SMOKE_IMAGES; do
        "$here/smoke-in-container.sh" "$image" "$archive" --dataframe-archive "$engine" --linalg-archive "$linalg"
    done
fi
echo "local-check: $archive"
