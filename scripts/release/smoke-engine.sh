#!/usr/bin/env bash
# Exercise an engine bundled in a compiler archive: default discovery,
# optional linkage, --dist output and app-only relocation.
# Usage: bash smoke-engine.sh ARCHIVE dataframe|linalg
# Needs only the compiler release's prerequisites; no Python/Rust/source tree.
set -euo pipefail

archive="$(realpath "$1")"
name="$2"
case "$name" in dataframe|linalg) ;; *) echo "unknown engine $name" >&2; exit 2 ;; esac
release="$(basename "$archive" .tar.gz)"
library="libplenty_${name}_v1.so"
fail() { echo "smoke-$name: FAIL: $*" >&2; exit 1; }
(cd "$(dirname "$archive")" && sha256sum -c "$(basename "$archive").sha256")
listing="$(tar -tvzf "$archive")"
for entry in "bin/$library" "bin/$name-notices/engine.json" "bin/$name-notices/LICENSE-MIT" \
    "bin/$name-notices/LICENSE-APACHE" "bin/$name-notices/THIRD-PARTY-NOTICES.md" \
    "bin/$name-notices/licenses/rust/COPYRIGHT-library.html"; do
    grep -Eq "^-rw-r--r-- 0/0 .* $release/$entry\$" <<<"$listing" || fail "missing or incorrect permissions: $entry"
done
work="$(mktemp -d "${TMPDIR:-/tmp}/plenty $name smoke.XXXXXX")"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/install" "$work/home" "$work/tmp" "$work/source" "$work/app"
tar -xzf "$archive" -C "$work/install"
compiler="$work/install/$release/bin/plenty"
as_user() {
    env -i HOME="$work/home" TMPDIR="$work/tmp" LANG=C.UTF-8 POLARS_MAX_THREADS=2 \
        PATH=/usr/local/bin:/usr/bin:/bin "$@"
}
cat > "$work/source/plain.plenty" <<EOF
import std.$name

def main() -> Result[(), IoError]:
    print("ordinary")?
    Ok(())
EOF
if [ "$name" = dataframe ]; then
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
expected=6
else
cat > "$work/source/analysis.plenty" <<'EOF'
import std.linalg as la

def main() -> Result[(), Failure]:
    a = la.matrix(2, 2, &[3.0, 1.0, 1.0, 2.0]?)?
    b = la.vector(&[9.0, 8.0]?)?
    x = a.solve(&b)?
    print(x.get(0, 0)?, x.get(1, 0)?)?
    Ok(())
EOF
expected='2.0 3.0'
fi
cd "$work/source"
as_user "$compiler" --compile plain.plenty -o "$work/app/plain"
if readelf -d "$work/app/plain" | grep "libplenty_$name"; then fail "unused import links the engine"; fi

# The bundled engine is found beside the compiler without configuration.
as_user "$compiler" --compile analysis.plenty -o "$work/app/ordinary"
needed="$(readelf -d "$work/app/ordinary")"
grep -q "(NEEDED).*\[$library\]" <<<"$needed" || fail "application lacks versioned library dependency"
grep -q '\$ORIGIN' <<<"$needed" || fail "application lacks relocatable search path"
[ "$(cd "$work/app" && as_user ./ordinary)" = "$expected" ] || fail "ordinary output result"
as_user "$compiler" --compile analysis.plenty --dist "$work/dist"
for entry in analysis "$library" "$name-notices/engine.json" "$name-notices/THIRD-PARTY-NOTICES.md"; do
    [ -f "$work/dist/$entry" ] || fail "--dist output lacks $entry"
done

# Remove the compiler and source, then relocate the distribution.
cd "$work"
rm -rf "$work/install" "$work/source"
mv "$work/dist" "$work/moved app"
cd "$work/moved app"
rm -f smoke.parquet
[ "$(as_user ./analysis)" = "$expected" ] || fail "--dist result after relocation"
mv "$library" "$library.hidden"
if as_user ./analysis > "$work/error" 2>&1; then fail "application ran without its engine"; fi
grep -q "$library" "$work/error" || fail "missing runtime engine diagnostic"
[ "$(as_user "$work/app/plain")" = ordinary ] || fail "ordinary program requires the engine"
echo "smoke-$name: PASS $release (bundled engine, --dist relocation, optional linkage)"
