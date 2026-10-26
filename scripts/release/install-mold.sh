#!/usr/bin/env bash
# Install the pinned x86-64 Linux linker without root. The caller adds
# DIR/bin to PATH and sets COMPILER_PATH=DIR/libexec/mold.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=/dev/null
. "$here/toolchain.env"
[ "$#" = 1 ] || { echo "usage: install-mold.sh DIR" >&2; exit 2; }

archive="$(mktemp "${TMPDIR:-/tmp}/plenty-mold.XXXXXX")"
trap 'rm -f "$archive"' EXIT
curl --proto '=https' --tlsv1.2 -sSfL -o "$archive" \
    "https://github.com/rui314/mold/releases/download/v$MOLD_VERSION/mold-$MOLD_VERSION-x86_64-linux.tar.gz"
echo "$MOLD_SHA256  $archive" | sha256sum -c -
mkdir -p "$1"
tar -xzf "$archive" --strip-components=1 -C "$1"
"$1/bin/mold" --version
