#!/usr/bin/env bash
# Exercise optional installation and app-only relocation using release assets.
# Usage: bash smoke-dataframe.sh COMPILER_ARCHIVE ENGINE_ARCHIVE
# Needs only the compiler release's prerequisites; no Python/Rust/source tree.
set -euo pipefail

compiler="$(realpath "$1")"
engine="$(realpath "$2")"
compiler_name="$(basename "$compiler" .tar.gz)"
engine_name="$(basename "$engine" .tar.gz)"
library=libplenty_dataframe_v1.so
fail() { echo "smoke-dataframe: FAIL: $*" >&2; exit 1; }
[ "$engine_name" = "plenty-dataframe-${compiler_name#plenty-}" ] || fail "compiler and engine releases differ"
for archive in "$compiler" "$engine"; do
    (cd "$(dirname "$archive")" && sha256sum -c "$(basename "$archive").sha256")
done
tops="$(tar -tzf "$engine" | cut -d/ -f1 | sort -u)"
[ "$tops" = "$engine_name" ] || fail "unexpected engine layout"
listing="$(tar -tvzf "$engine")"
for entry in "lib/$library" engine.json README.md LICENSE-MIT LICENSE-APACHE THIRD-PARTY-NOTICES.md licenses/rust/COPYRIGHT-library.html; do
    grep -Eq "^-rw-r--r-- 0/0 .* $engine_name/$entry\$" <<<"$listing" || fail "missing or incorrect permissions: $entry"
done
if grep -Ev '^(-rw-r--r--|drwxr-xr-x) 0/0 ' <<<"$listing"; then
    fail "unexpected modes/owners in engine archive"
fi
work="$(mktemp -d "${TMPDIR:-/tmp}/plenty dataframe smoke.XXXXXX")"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/install" "$work/home" "$work/tmp" "$work/source" "$work/app"
tar -xzf "$compiler" -C "$work/install"
tar -xzf "$engine" -C "$work/install"
compiler_root="$work/install/$compiler_name"
engine_root="$work/install/$engine_name"
as_user() {
    env -i HOME="$work/home" TMPDIR="$work/tmp" LANG=C.UTF-8 POLARS_MAX_THREADS=2 \
        PATH=/usr/local/bin:/usr/bin:/bin "$@"
}
cat > "$work/source/plain.plenty" <<'EOF'
import std.dataframe

def main() -> Result[(), IoError]:
    print("ordinary")?
    Ok(())
EOF
cat > "$work/source/analysis.plenty" <<'EOF'
import std.dataframe as df

def main() -> Result[(), Failure]:
    columns = [df.column_i64("value", [1, 2, 3]?)?]?
    data = df.frame(columns)?
    data.write_parquet("smoke.parquet")?
    totals = [df.col("value")?.sum()?.alias("total")?]?
    result = df.scan_parquet("smoke.parquet")?.select(totals)?.collect_streaming()?
    print(result.get_i64("total", 0)?.unwrap())?
    Ok(())
EOF
cd "$work/source"
as_user "$compiler_root/bin/plenty" --compile plain.plenty -o "$work/app/plain"
if readelf -d "$work/app/plain" | grep 'libplenty_dataframe'; then fail "unused dataframe import links the engine"; fi
as_user "$compiler_root/bin/plenty" --check analysis.plenty
if as_user "$compiler_root/bin/plenty" --compile analysis.plenty -o "$work/app/analysis" > "$work/error" 2>&1; then
    fail "linking without engine succeeded"
fi
grep -q 'requires the optional Polars engine' "$work/error" || fail "missing-engine diagnostic"

# Explicit path, then automatic discovery beside the compiler.
as_user env PLENTY_DATAFRAME_ENGINE="$engine_root/lib/$library" \
    "$compiler_root/bin/plenty" --compile analysis.plenty -o "$work/app/explicit"
cp "$engine_root/lib/$library" "$compiler_root/bin/"
as_user "$compiler_root/bin/plenty" --compile analysis.plenty -o "$work/app/analysis"
needed="$(readelf -d "$work/app/analysis")"
grep -q "(NEEDED).*\[$library\]" <<<"$needed" || fail "application lacks versioned library dependency"
grep -q '\$ORIGIN' <<<"$needed" || fail "application lacks relocatable search path"
cp "$engine_root/lib/$library" "$work/app/"
mkdir "$work/app/dataframe-notices"
cp "$engine_root/engine.json" "$engine_root/LICENSE-MIT" "$engine_root/LICENSE-APACHE" \
    "$engine_root/THIRD-PARTY-NOTICES.md" "$work/app/dataframe-notices/"
cp -R "$engine_root/licenses" "$work/app/dataframe-notices/"

# Remove every build-time library path, source and compiler, then relocate.
cd "$work"
rm -rf "$work/install" "$work/source"
mv "$work/app" "$work/moved app"
cd "$work/moved app"
for exe in analysis explicit; do
    rm -f smoke.parquet
    [ "$(as_user "./$exe")" = 6 ] || fail "$exe result after relocation"
done
mv "$library" "$library.hidden"
[ "$(as_user ./plain)" = ordinary ] || fail "ordinary program requires the engine"
if as_user ./analysis > "$work/error" 2>&1; then fail "application ran without its engine"; fi
grep -q "$library" "$work/error" || fail "missing runtime engine diagnostic"
echo "smoke-dataframe: PASS $engine_name (app-only relocation, optional linkage)"
