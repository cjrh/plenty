#!/usr/bin/env bash
# Package already-built Plenty executables as a release archive.
#
# Usage: package.sh --bin-dir DIR --out-dir DIR [--rust-sysroot DIR]
#                   [--allow-missing-license]
#
# Writes OUT/plenty-VERSION-TARGET.tar.gz and its .sha256 file, with this layout:
#
#   plenty-VERSION-TARGET/
#     bin/plenty, bin/plentyc
#     README.md                 (scripts/release/README.dist.md, filled in)
#     LICENSE                   (the repository's LICENSE)
#     THIRD-PARTY-NOTICES.md    (third_party_notices.py)
#     licenses/rust/            (the Rust standard library's notices)
#
# VERSION is read from Cargo.toml and TARGET from `plenty --version`, and the
# two must agree with the executable. The archive is reproducible: entries are
# sorted, owned by 0:0, and dated SOURCE_DATE_EPOCH (default: the HEAD commit
# time, else 0). --allow-missing-license is for local layout checks only; a
# release must ship the project's license.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
bin_dir=""
out_dir=""
sysroot=""
allow_missing_license=0
while [ $# -gt 0 ]; do
    case "$1" in
        --bin-dir) bin_dir="$2"; shift 2 ;;
        --out-dir) out_dir="$2"; shift 2 ;;
        --rust-sysroot) sysroot="$2"; shift 2 ;;
        --allow-missing-license) allow_missing_license=1; shift ;;
        *) echo "package.sh: unknown argument $1" >&2; exit 2 ;;
    esac
done
[ -n "$bin_dir" ] && [ -n "$out_dir" ] || { echo "usage: package.sh --bin-dir DIR --out-dir DIR" >&2; exit 2; }
[ -n "$sysroot" ] || sysroot="$(rustc --print sysroot)"

version="$("$here/check-version.sh" --print)"
report="$("$bin_dir/plenty" --version)"
target="${report##*(}"
target="${target%)}"
if [ "$report" != "plenty $version ($target)" ]; then
    echo "package.sh: $bin_dir/plenty reports '$report', expected version $version" >&2
    exit 1
fi
if [ "$target" != "x86_64-unknown-linux-gnu" ]; then
    echo "package.sh: unsupported release target $target" >&2
    exit 1
fi

name="plenty-$version-$target"
mkdir -p "$out_dir"
out_dir="$(cd "$out_dir" && pwd)"
stage="$(mktemp -d "${TMPDIR:-/tmp}/plenty-package.XXXXXX")"
trap 'rm -rf "$stage"' EXIT
root="$stage/$name"
mkdir -p "$root/bin" "$root/licenses/rust"

install -m 0755 "$bin_dir/plenty" "$bin_dir/plentyc" "$root/bin/"
sed -e "s/@VERSION@/$version/g" -e "s/@TARGET@/$target/g" \
    "$here/README.dist.md" > "$root/README.md"
if [ -f "$repo/LICENSE" ]; then
    install -m 0644 "$repo/LICENSE" "$root/LICENSE"
elif [ "$allow_missing_license" = 1 ]; then
    echo "package.sh: warning: no LICENSE in the repository; this archive is not releasable" >&2
else
    echo "package.sh: no LICENSE in the repository; a release must ship the project's license" >&2
    exit 1
fi
python3 -B "$here/third_party_notices.py" --repo "$repo" --out "$root/THIRD-PARTY-NOTICES.md"
rust_docs="$sysroot/share/doc/rust"
if [ ! -f "$rust_docs/COPYRIGHT-library.html" ]; then
    echo "package.sh: $rust_docs/COPYRIGHT-library.html not found; install the rustc component's docs" >&2
    exit 1
fi
install -m 0644 "$rust_docs/COPYRIGHT-library.html" "$rust_docs"/licenses/*.txt "$root/licenses/rust/"
chmod 0644 "$root/README.md" "$root/THIRD-PARTY-NOTICES.md"
find "$root" -type d -exec chmod 0755 {} +

if [ -z "${SOURCE_DATE_EPOCH:-}" ]; then
    SOURCE_DATE_EPOCH="$(git -C "$repo" log -1 --format=%ct 2>/dev/null || echo 0)"
fi
archive="$name.tar.gz"
tar --sort=name --mtime="@$SOURCE_DATE_EPOCH" --owner=0 --group=0 --numeric-owner \
    --format=gnu -C "$stage" -cf - "$name" | gzip -n -9 > "$out_dir/$archive"
(cd "$out_dir" && sha256sum "$archive" > "$archive.sha256")
echo "$out_dir/$archive"
