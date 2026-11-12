#!/usr/bin/env bash
# Lint and check the AArch64 Linux build from an x86-64 host, to catch code
# that only compiles or lints cleanly on x86-64 before CI's ARM runner does.
# Needs the Rust target (`rustup target add aarch64-unknown-linux-gnu`) and a C
# cross-compiler for build scripts: `aarch64-linux-gnu-gcc`, or `zig`, which
# this script wraps when the GNU cross-compiler is missing.
set -euo pipefail

target=aarch64-unknown-linux-gnu
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

if ! command -v aarch64-linux-gnu-gcc >/dev/null; then
    command -v zig >/dev/null || {
        echo "arm_lint: install aarch64-linux-gnu-gcc or zig" >&2
        exit 2
    }
    tools="$root/target/arm-lint-tools"
    mkdir -p "$tools"
    for pair in gcc:cc g++:c++; do
        # cc-rs passes the Rust triple as --target, which zig rejects.
        cat >"$tools/aarch64-linux-gnu-${pair%%:*}" <<WRAPPER
#!/usr/bin/env bash
args=()
for arg in "\$@"; do
    case "\$arg" in --target=*) ;; *) args+=("\$arg") ;; esac
done
exec zig ${pair##*:} -target aarch64-linux-gnu "\${args[@]}"
WRAPPER
    done
    printf '#!/bin/sh\nexec zig ar "$@"\n' >"$tools/aarch64-linux-gnu-ar"
    chmod +x "$tools"/aarch64-linux-gnu-*
    PATH="$tools:$PATH"
fi

# The production build enables only the native Cranelift backend; test builds
# enable x86-64 too, which hides imports that exist only there.
cargo check --locked --workspace --target "$target"
cargo clippy --locked --workspace --all-targets --target "$target" -- -D warnings
