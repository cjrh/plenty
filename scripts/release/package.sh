#!/usr/bin/env bash
# Package already-built Plenty executables as a release archive.
#
# Usage: package.sh --bin-dir DIR --out-dir DIR [--rust-sysroot DIR]
#
# Writes OUT/plenty-VERSION-TARGET.tar.gz (.zip on Windows) and its .sha256
# file, with this layout:
#
#   plenty-VERSION-TARGET/
#     bin/plenty, bin/plentyc   (plenty.exe, plentyc.exe on Windows)
#     README.md                 (README.dist.md, or README.dist-OS.md, filled in)
#     LICENSE-MIT, LICENSE-APACHE  (Plenty is MIT OR Apache-2.0)
#     THIRD-PARTY-NOTICES.md    (third_party_notices.py)
#     licenses/rust/            (the Rust standard library's notices)
#
# VERSION is read from Cargo.toml and TARGET from `plenty --version`, and the
# two must agree with the executable. The archive is reproducible: entries are
# sorted, owned by 0:0, and dated SOURCE_DATE_EPOCH (default: the HEAD commit
# time, else 0). Linux archives use GNU tar; macOS and Windows hosts lack it,
# so archive.py writes theirs. PYTHON selects the interpreter (default python3).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
bin_dir=""
out_dir=""
sysroot=""
while [ $# -gt 0 ]; do
    case "$1" in
        --bin-dir) bin_dir="$2"; shift 2 ;;
        --out-dir) out_dir="$2"; shift 2 ;;
        --rust-sysroot) sysroot="$2"; shift 2 ;;
        *) echo "package.sh: unknown argument $1" >&2; exit 2 ;;
    esac
done
[ -n "$bin_dir" ] && [ -n "$out_dir" ] || { echo "usage: package.sh --bin-dir DIR --out-dir DIR" >&2; exit 2; }
[ -n "$sysroot" ] || sysroot="$(rustc --print sysroot)"
python="${PYTHON:-python3}"
# Git Bash on Windows: rustc prints a native path.
if command -v cygpath >/dev/null 2>&1; then sysroot="$(cygpath -u "$sysroot")"; fi
sha256() { if command -v sha256sum >/dev/null 2>&1; then sha256sum "$@"; else shasum -a 256 "$@"; fi; }

version="$("$here/check-version.sh" --print)"
exe=""
if [ -f "$bin_dir/plenty.exe" ]; then exe=.exe; fi
report="$("$bin_dir/plenty$exe" --version)"
target="${report##*(}"
target="${target%)}"
if [ "$report" != "plenty $version ($target)" ]; then
    echo "package.sh: $bin_dir/plenty reports '$report', expected version $version" >&2
    exit 1
fi
PLENTY_RELEASE_TARGET="$target"
# shellcheck source=target.sh
. "$here/target.sh"
[ "$exe" = "$release_exe" ] || { echo "package.sh: $target executables must end in '$release_exe'" >&2; exit 1; }
if [ "$release_os" = linux ]; then
    "$here/check-elf.sh" "$bin_dir/plenty" "$bin_dir/plentyc"
    readme="$here/README.dist.md"
else
    "$python" -B "$here/check-native.py" --target "$target" --macos-min "${release_macos_min:-11.0}" \
        "$bin_dir/plenty$exe" "$bin_dir/plentyc$exe"
    readme="$here/README.dist-$release_os.md"
fi

name="plenty-$version-$target"
mkdir -p "$out_dir"
out_dir="$(cd "$out_dir" && pwd)"
stage="$(mktemp -d "${TMPDIR:-/tmp}/plenty-package.XXXXXX")"
trap 'rm -rf "$stage"' EXIT
root="$stage/$name"
mkdir -p "$root/bin" "$root/licenses/rust"

install -m 0755 "$bin_dir/plenty$exe" "$bin_dir/plentyc$exe" "$root/bin/"
sed -e "s/@VERSION@/$version/g" -e "s/@TARGET@/$target/g" "$readme" > "$root/README.md"
install -m 0644 "$repo/LICENSE-MIT" "$repo/LICENSE-APACHE" "$root/"
"$python" -B "$here/third_party_notices.py" --repo "$repo" --target "$target" --out "$root/THIRD-PARTY-NOTICES.md"
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
archive="$name.$release_archive_ext"
if [ "$release_os" = linux ]; then
    tar --sort=name --mtime="@$SOURCE_DATE_EPOCH" --owner=0 --group=0 --numeric-owner \
        --format=gnu -C "$stage" -cf - "$name" | gzip -n -9 > "$out_dir/$archive"
else
    SOURCE_DATE_EPOCH="$SOURCE_DATE_EPOCH" "$python" -B "$here/archive.py" "$stage" "$name" "$out_dir/$archive"
fi
(cd "$out_dir" && sha256 "$archive" > "$archive.sha256")
echo "$out_dir/$archive"
