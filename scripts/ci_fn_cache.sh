#!/usr/bin/env bash
# Keep CI's persisted compiled-function cache to the entries the current
# test run used (issue #403). See book/src/development/workflow.md.
#
#   ci_fn_cache.sh mark DIR       after restoring DIR, before the tests
#   ci_fn_cache.sh prune DIR KEY_PREFIX
#                                 after the tests: delete unused entries and
#                                 write key=KEY_PREFIX<digest of the entry set>
#                                 to $GITHUB_OUTPUT (stdout outside Actions)
#
# A cache hit only reads its entry file (src/cache.rs), so use shows only in
# the access time. `mark` sets every restored entry's access and modification
# time to the same old instant; under the kernel's default `relatime`, the
# next read of such a file sets its access time to now. `prune` then deletes
# every entry whose access time is still older than the mark. Entries the
# tests wrote are new and stay.
#
# If the filesystem does not record reads (`noatime`), `mark` says so and
# `prune` keeps everything and writes no key, so nothing is saved.
set -euo pipefail

old=@946684800 # 2000-01-01, older than any real write

case "${1:-}" in
mark)
    dir=$2
    mkdir -p "$dir"
    find "$dir" -type f -exec touch -h -d "$old" {} +
    probe="$dir.atime-probe"
    echo probe >"$probe"
    touch -h -d "$old" "$probe"
    before=$(stat -c %X "$probe")
    cat "$probe" >/dev/null
    after=$(stat -c %X "$probe")
    rm -f "$probe"
    if [ "$before" = "$after" ]; then
        echo "::warning::reads do not update access times here; the compiled-function cache will not be pruned or saved"
        rm -f "$dir.mark"
    else
        touch "$dir.mark"
    fi
    echo "restored entries: $(find "$dir" -type f | wc -l), $(du -sh "$dir" | cut -f1)"
    ;;
prune)
    dir=$2
    prefix=$3
    if [ ! -e "$dir.mark" ]; then
        echo "no usable mark; keeping the cache unchanged and not saving it"
        exit 0
    fi
    find "$dir" -type f \( -name '*.tmp' -o ! -anewer "$dir.mark" \) -print0 |
        xargs -0 -r rm -f --
    find "$dir" -mindepth 1 -type d -empty -delete
    count=$(find "$dir" -type f | wc -l)
    echo "kept entries: $count, $(du -sh "$dir" | cut -f1)"
    if [ "$count" -eq 0 ]; then
        exit 0
    fi
    # Entry files are named by a hash of their key, so the set of names
    # identifies the cache's contents.
    digest=$(find "$dir" -type f -printf '%f\n' | LC_ALL=C sort | sha256sum | cut -c1-16)
    echo "key=$prefix$digest" >>"${GITHUB_OUTPUT:-/dev/stdout}"
    ;;
*)
    echo "usage: $0 mark DIR | prune DIR KEY_PREFIX" >&2
    exit 2
    ;;
esac
