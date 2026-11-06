#!/usr/bin/env bash
# Run every CI and release check this Linux host can run, then summarize.
#
# Usage: local_ci.sh [--release] [STEP...]
#
# With no STEP, runs every host step below in order. Each step's output goes to
# target/local-ci/run-*/STEP.log; a failing step does not stop later steps, and the
# exit status is nonzero if any failed. --release also runs the release
# workflow's glibc 2.28 container build, full test suite, and distribution
# smoke tests (scripts/release/local-check.sh --test; needs Docker).
#
# Steps mirror .github/workflows/ci.yml, portability.yml, docs.yml and the
# release workflow's Linux jobs. Not covered: the macOS and Windows jobs
# (native builds, DLL/Mach-O audits, their unit tests), the other Linux
# architecture, and the nightly toolchain. `notices` does check every target's
# license notices, which need no native build.
set -uo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo" || exit 1
logs="$repo/target/local-ci"
mkdir -p "$logs/tmp"
# Build-heavy tests overflow a small /tmp quota.
export TMPDIR="$logs/tmp"

step_fmt() { cargo fmt --all -- --check; }

step_mold() { scripts/ci_check_mold.sh; }

# Build-tool and crypto-source selection tests, run by lint and portability.
step_native_tools() {
    mkdir -p target/native-tools || return
    rustc --edition 2024 --test plenty-runtime/native/tools.rs -o target/native-tools/tests &&
        target/native-tools/tests &&
        rustc --edition 2024 --test plenty-runtime/native/ring_sources.rs -o target/native-tools/ring-sources &&
        target/native-tools/ring-sources
}

step_platform() { cargo test --locked --manifest-path plenty-runtime/platform-tests/Cargo.toml; }

step_clippy() { cargo clippy --locked --workspace --all-targets -- -D warnings; }

# The doctests job also checks that default features build and link.
step_default_build() { cargo build --locked --workspace; }

# Includes doctests, which the nextest shards skip.
step_test() { cargo test --locked --workspace --all-features; }

step_proxy() {
    command -v docker >/dev/null || { echo "docker is required"; return 1; }
    docker pull nginx:1.30.6-alpine3.24 && docker pull caddy:2.11.7-alpine &&
        PLENTY_DOCKER_TESTS=1 cargo test --locked --test integration test_std_http_server_proxy::
}

step_engine() {
    local engine=$1 upper=${1^^}
    python3 -B scripts/build_native_engine.py "$engine" --out target/engine --validate || return
    local -x "PLENTY_${upper}_TESTS=1" "PLENTY_${upper}_ENGINE=$repo/target/engine/libplenty_${engine}_v1.so"
    cargo test --locked --features runtime-checks --test integration "test_std_${engine}::" &&
        cargo test --locked --test integration "test_tutorial::${engine}_examples_with_optional_engine" || return
    if [ "$engine" = dataframe ]; then
        cargo build --locked --bin plenty &&
            python3 -B examples/sales_report/check.py --plenty target/debug/plenty -v
    fi
}
step_dataframe() { step_engine dataframe; }
step_linalg() { step_engine linalg; }

# Every target's notices: a dependency new to one platform must still have
# reviewed license text (the macOS objc2 failure in 0.8.4).
step_notices() {
    local status=0 target component
    for target in x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu aarch64-apple-darwin x86_64-pc-windows-msvc; do
        for component in compiler dataframe linalg; do
            echo "== $target $component"
            python3 -B scripts/release/third_party_notices.py --repo . --target "$target" \
                --component "$component" --out "$TMPDIR/notices-$target-$component.md" || status=1
        done
    done
    return $status
}

step_scripts() { python3 -B -m unittest discover -s scripts -p 'test_*.py'; }

step_links() { python3 -B scripts/check_links.py book/src; }

step_docs_site() { python3 -B scripts/build_docs_site.py; }

step_release() { scripts/release/local-check.sh --test; }

all=(fmt mold native_tools platform clippy default_build test proxy dataframe linalg notices scripts links docs_site)

steps=()
for arg in "$@"; do
    case "$arg" in
        --release) release=1 ;;
        -h | --help) sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'; echo "Steps: ${all[*]} release"; exit 0 ;;
        *)
            declare -F "step_${arg//-/_}" >/dev/null || { echo "local_ci.sh: unknown step $arg" >&2; exit 2; }
            steps+=("${arg//-/_}")
            ;;
    esac
done
[ ${#steps[@]} -gt 0 ] || steps=("${all[@]}")
[ -z "${release:-}" ] || steps+=(release)

timing_dir="$(mktemp -d "$logs/run-$(date -u +%Y%m%dT%H%M%S)-XXXXXX")" || exit 1
timing_rows="$timing_dir/steps.tsv"
: > "$timing_rows"
run_start=$SECONDS
failed=()
summary=()
for step in "${steps[@]}"; do
    log="$timing_dir/$step.log"
    start=$SECONDS
    printf '%-14s ... ' "$step"
    if ("step_$step") >"$log" 2>&1; then result=ok; else result=FAILED; failed+=("$step"); fi
    line="$(printf '%-14s %-6s %5ss  %s' "$step" "$result" "$((SECONDS - start))" "${log#"$repo"/}")"
    echo "$result ($((SECONDS - start))s)"
    summary+=("$line")
    printf '%s\t%s\t%s\n' "$step" "$result" "$((SECONDS - start))" >> "$timing_rows"
done

echo
printf '%s\n' "${summary[@]}"
python3 -B scripts/check_timings.py local "$timing_rows" --elapsed "$((SECONDS - run_start))" --output "$timing_dir" || exit 1
if [ ${#failed[@]} -gt 0 ]; then
    echo "local-ci: failed: ${failed[*]}"
    exit 1
fi
echo "local-ci: all ${#steps[@]} steps passed (host-only; macOS and Windows jobs not covered)"
