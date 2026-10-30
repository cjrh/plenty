#!/usr/bin/env bash
# Check a release archive as a user would install it.
#
# Usage: smoke-test.sh ARCHIVE [--clean-environment]
#
# ARCHIVE.sha256 must sit next to ARCHIVE. The script checks the checksum,
# layout, and permissions; extracts into a fresh directory whose path contains
# spaces; puts only its bin/ and the system directories on PATH; and, from an
# unrelated directory, reports the version, compiles and runs programs that
# import embedded standard-library modules and native-backed facilities
# (SQLite, SHA-256), runs a project, and checks the missing-linker error. It
# then moves and renames the installation, installs a second copy side by side,
# and repeats. The installation must be unchanged afterwards: caches and
# temporary files go to HOME and TMPDIR.
#
# --clean-environment additionally requires that no Rust toolchain, Cargo
# home, or source checkout is reachable, as in the release smoke containers.
set -euo pipefail

archive="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
clean=0
required="bin/plenty bin/plentyc README.md LICENSE-MIT LICENSE-APACHE THIRD-PARTY-NOTICES.md licenses/rust/COPYRIGHT-library.html"
for option in "${@:2}"; do
    case "$option" in
        --clean-environment) clean=1 ;;
        *) echo "smoke-test: unknown option $option" >&2; exit 2 ;;
    esac
done
file="$(basename "$archive")"
name="${file%.tar.gz}"
version="${name#plenty-}"
case "$name" in
    *-x86_64-unknown-linux-gnu) target=x86_64-unknown-linux-gnu ;;
    *-aarch64-unknown-linux-gnu) target=aarch64-unknown-linux-gnu ;;
    *) echo "smoke-test: unsupported archive target: $name" >&2; exit 1 ;;
esac
version="${version%-"$target"}"

fail() { echo "smoke-test: FAIL: $*" >&2; exit 1; }
step() { echo "smoke-test: $*"; }

[ "$file" = "plenty-$version-$target.tar.gz" ] || fail "unexpected archive name $file"
step "checksum"
(cd "$(dirname "$archive")" && sha256sum -c "$file.sha256")

step "layout and permissions"
listing="$(tar -tvzf "$archive")"
tops="$(tar -tzf "$archive" | cut -d/ -f1 | sort -u)"
[ "$tops" = "$name" ] || fail "top-level entries: $tops"
for entry in $required; do
    grep -q " $name/$entry\$" <<<"$listing" || fail "missing $entry"
done
for exe in plenty plentyc; do
    grep -Eq "^-rwxr-xr-x 0/0 .* $name/bin/$exe\$" <<<"$listing" || fail "bin/$exe is not 0755 root-owned"
done
if grep -Ev "^(-rw-r--r--|-rwxr-xr-x|drwxr-xr-x) 0/0 " <<<"$listing"; then
    fail "entries above have unexpected modes or owners"
fi

work="$(mktemp -d "${TMPDIR:-/tmp}/plenty smoke.XXXXXX")"
trap 'chmod -R u+w "$work" 2>/dev/null; rm -rf "$work"' EXIT
mkdir -p "$work/Downloads/my releases" "$work/home" "$work/tmp" "$work/elsewhere/a project"
tar -xzf "$archive" -C "$work/Downloads/my releases"
install="$work/Downloads/my releases/$name"
system_path=/usr/local/bin:/usr/bin:/bin

# Run a command as a user with only `$1/bin` added to the system PATH.
as_user() {
    local root="$1"; shift
    env -i HOME="$work/home" TMPDIR="$work/tmp" LANG=C.UTF-8 \
        PATH="$root/bin:$system_path" "$@"
}

if [ "$clean" = 1 ]; then
    step "clean environment"
    for tool in cargo rustc rustup; do
        if as_user "$install" sh -c "command -v $tool" >/dev/null; then
            fail "$tool is on PATH"
        fi
    done
    [ ! -e "$HOME/.cargo" ] && [ ! -e "$HOME/.rustup" ] || fail "a Rust toolchain is installed in $HOME"
    [ ! -e /usr/lib/rustlib ] && [ ! -e /usr/local/lib/rustlib ] || fail "a system Rust is installed"
    if as_user "$install" sh -c 'command -v cc' >/dev/null; then :; else
        fail "the documented prerequisite cc is missing"
    fi
fi

snapshot() { (cd "$1" && find . -printf '%p %m %s\n' | sort && find . -type f -exec sha256sum {} + | sort); }
before="$(snapshot "$install")"

cat > "$work/elsewhere/a project/smoke.plenty" <<'EOF'
import std.bytes
import std.crypto
import std.db.sqlite
import std.encoding

def main() -> Result[(), Failure]:
    data = std.bytes.from_text("abc")?
    digest = std.crypto.sha256(&data)?
    print(std.encoding.hex_text(&digest, 0, digest.len())?)?
    mut db = std.db.sqlite.memory(65536)?
    db.execute("CREATE TABLE t(name TEXT NOT NULL, n INTEGER)")?
    mut insert = db.prepare("INSERT INTO t VALUES (?, ?)")?
    insert.bind_text(1, "relocated")?
    insert.bind_integer(2, 42)?
    insert.run()?
    mut query = db.prepare("SELECT name, n FROM t")?
    while query.step()?:
        print(query.text(0)?, query.integer(1)?)?
    Ok(())
EOF
expected="ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
relocated 42"

exercise() {
    local root="$1" label="$2"
    step "$label: version"
    local reported
    reported="$(cd "$work/elsewhere" && as_user "$root" plenty --version)"
    [ "$reported" = "plenty $version ($target)" ] || fail "plenty --version: $reported"
    reported="$(cd "$work/elsewhere" && as_user "$root" plentyc --version)"
    [ "$reported" = "plentyc $version ($target)" ] || fail "plentyc --version: $reported"
    [ "$(as_user "$root" sh -c 'command -v plenty')" = "$root/bin/plenty" ] || fail "PATH does not select $root"

    step "$label: compile and run from another directory"
    (
        cd "$work/elsewhere/a project"
        rm -f "smoke program"
        as_user "$root" plenty --compile smoke.plenty -o "smoke program"
        out="$(as_user "$root" "./smoke program")"
        [ "$out" = "$expected" ] || fail "compiled program printed: $out"
        out="$(as_user "$root" plenty smoke.plenty)"
        [ "$out" = "$expected" ] || fail "plenty FILE printed: $out"
    )

    step "$label: project"
    (
        cd "$work/elsewhere"
        rm -rf "new app"
        as_user "$root" plenty new "new app" --name new_app 2>/dev/null
        cd "new app"
        out="$(as_user "$root" plenty run 2>/dev/null)"
        [ -n "$out" ] || fail "plenty run printed nothing"
    )

    step "$label: missing linker"
    (
        cd "$work/elsewhere/a project"
        set +e
        err="$(env -i HOME="$work/home" TMPDIR="$work/tmp" PATH="$root/bin" \
            "$root/bin/plenty" --compile smoke.plenty -o unlinked 2>&1)"
        code=$?
        set -e
        [ "$code" != 0 ] || fail "compiling without cc succeeded"
        grep -q "install gcc or clang" <<<"$err" || fail "missing-linker error: $err"
        env -i HOME="$work/home" TMPDIR="$work/tmp" PATH="$root/bin" \
            "$root/bin/plenty" --check smoke.plenty || fail "--check needs a linker"
    )
}

exercise "$install" "extracted"

step "move and rename"
mkdir -p "$work/moved elsewhere"
mv "$install" "$work/moved elsewhere/plenty renamed"
moved="$work/moved elsewhere/plenty renamed"
exercise "$moved" "moved"

step "side by side"
mkdir -p "$work/second copy"
tar -xzf "$archive" -C "$work/second copy"
exercise "$work/second copy/$name" "second copy"
exercise "$moved" "first copy again"

step "installation unchanged"
after="$(snapshot "$moved")"
[ "$before" = "$after" ] || { diff <(echo "$before") <(echo "$after") >&2 || true; fail "the installation directory changed"; }

step "PASS $name"
