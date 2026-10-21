#!/usr/bin/env bash
# Repository convenience commands; corpus extraction and database work are Plenty.
set -euo pipefail
checkout=$(git -C "$(dirname "${BASH_SOURCE[0]}")/.." rev-parse --show-toplevel)
primary=$(git -C "$checkout" worktree list --porcelain | sed -n '1s/^worktree //p')
state=${PLENTY_SEARCH_HOME:-"$primary/.search"}
root=${PLENTY_SEARCH_ROOT:-"$primary"}
python="$state/venv/bin/python"
cache="$state/models"
program="$state/semantic-search"
helper="$checkout/scripts/search_embed.py"
action=${1:-status}
if (($#)); then shift; fi

if [[ "$action" == setup ]]; then
    mkdir -p "$state"
    if [[ ! -x "$python" ]]; then uv venv "$state/venv"; fi
    uv pip install --python "$python" sqlite-vec==0.1.9 fastembed==0.9.0
    "$python" "$helper" --setup "$cache"
    action=build
fi
if [[ "$action" == build ]]; then
    mkdir -p "$state"
    cd "$checkout"
    cargo run --locked --bin plenty -- --compile examples/semantic_search.plenty -o "$program"
    exit
fi
if [[ ! -x "$python" || ! -x "$program" ]]; then
    echo "Run $0 setup first (or build after changing the Plenty source)." >&2
    exit 1
fi
extension=$("$python" -c 'import sqlite_vec; print(sqlite_vec.loadable_path())')
model=$("$python" "$helper" --identity)
database="$state/search.db"
case "$action" in
    check)
        exec "$program" "$extension" "$database" "$model" check "$root" "$primary/issues.db"
        ;;
    sync)
        if [[ ! -f "$database" ]]; then
            "$program" "$extension" "$database" "$model" init 384
        fi
        exec "$program" "$extension" "$database" "$model" sync "$root" "$primary/issues.db" "$python" "$helper" "$cache"
        ;;
    find)
        if (($# != 1)); then echo "usage: $0 find 'query text'" >&2; exit 1; fi
        exec "$program" "$extension" "$database" "$model" find "$1" "$python" "$helper" "$cache"
        ;;
    *) exec "$program" "$extension" "$database" "$model" "$action" "$@" ;;
esac
