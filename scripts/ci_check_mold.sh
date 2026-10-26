#!/usr/bin/env bash
# Fail unless `cc` links with mold under COMPILER_PATH, defaulting to
# .cargo/config.toml's system installation (issue #402). Release tests use
# their pinned rootless installation. Missing linkers must not fall back.
set -euo pipefail

linker_dir="${COMPILER_PATH:-/usr/libexec/mold}"
ld="$linker_dir/ld"
if [ ! -x "$ld" ]; then
    echo "::error::$ld is missing; test programs would link with GNU ld"
    exit 1
fi
"$ld" --version

dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
printf 'int main(void) { return 0; }\n' >"$dir/main.c"
COMPILER_PATH="$linker_dir" cc "$dir/main.c" -o "$dir/main"
# mold records itself in the .comment section of what it links.
if ! readelf -p .comment "$dir/main" | grep -c mold >/dev/null; then
    echo "::error::cc with COMPILER_PATH=$linker_dir did not link with mold"
    exit 1
fi
echo "cc links with mold"
