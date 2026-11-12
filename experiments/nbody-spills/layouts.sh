#!/bin/sh
# Instructions and cycles per step of n-body under two compilers, with 0 to 7
# statements added to `energy`, which precedes `advance`, to move the loop.
# Shows whether a difference is a placement effect (issue #437).
# Usage: layouts.sh BEFORE_PLENTY AFTER_PLENTY WORKDIR [CORE] [STEPS]
set -eu
here=$(dirname "$0")
before=$1
after=$2
work=$3
core=${4:-2}
steps=${5:-5000000}
source_file="$here/../../benchmarks/game/n-body/plenty/nbody.plenty"
mkdir -p "$work"
echo "statements before_instructions before_cycles after_instructions after_cycles advance_address_after"
for appends in $(seq 0 7); do
    source="$work/layout$appends.plenty"
    awk -v n="$appends" '/^    Ok\(e\)$/ {
            for (i = 0; i < n; i++) print "    e += 0.0 * (dt + " i ".0)"
        } { print }' "$source_file" | sed 's/^def energy(bodies: &list\[Body\])/def energy(bodies: \&list[Body], dt: f64)/; s/energy(&bodies)/energy(\&bodies, 0.01)/' > "$source"
    address=$(PLENTY_CACHE=off "$after" --no-strip --compile "$source" -o "$work/layout" && nm "$work/layout" | awk '/__plenty_fn_advance$/ { print $1 }')
    line="$appends"
    for compiler in "$before" "$after"; do
        PLENTY_CACHE=off "$compiler" --compile "$source" -o "$work/layout"
        counts=$(perf stat -x, -r 3 -e instructions:u,cycles:u taskset -c "$core" "$work/layout" "$steps" 2>&1 >/dev/null |
            cut -d, -f1 | awk -v s="$steps" '{ printf "%.1f ", $1 / s }')
        line="$line $counts"
    done
    echo "$line $address"
done
