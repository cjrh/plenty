#!/usr/bin/env bash
# Run from the repository root. The optional argument is a fresh output directory.
set -euo pipefail
work=${1:-target/knucleotide-codegen}
mkdir -p "$work"
rustc -C opt-level=3 -C target-cpu=native -C codegen-units=1 --emit=llvm-ir,asm,link benchmarks/game/k-nucleotide/reference/knucleotide.rs -o "$work/rust-native"
# Valgrind 3.27.1 rejects this machine's native AVX-512 code. Label the baseline separately.
rustc -g -C opt-level=3 -C target-cpu=x86-64 -C codegen-units=1 benchmarks/game/k-nucleotide/reference/knucleotide.rs -o "$work/rust-baseline"
rustc -C opt-level=3 benchmarks/game/fasta/reference/fasta.rs -o "$work/fasta"
"$work/fasta" 50000 > "$work/input.txt"
PLENTY_CACHE=off target/release/plenty --compile --no-strip benchmarks/game/k-nucleotide/plenty/knucleotide.plenty -o "$work/plenty"
for variant in rust-native rust-baseline plenty; do
    perf stat -e instructions:u -o "$work/$variant.perf" taskset -c 0,2,4,6,8,10,12 "$work/$variant" < "$work/input.txt" > "$work/$variant.out"
done
cmp "$work/plenty.out" "$work/rust-native.out"
cmp "$work/plenty.out" "$work/rust-baseline.out"
for variant in rust-baseline plenty; do
    valgrind --tool=callgrind --callgrind-out-file="$work/$variant.cg" "$work/$variant" < "$work/input.txt" > "$work/$variant-cg.out"
    cmp "$work/$variant.out" "$work/$variant-cg.out"
    callgrind_annotate --auto=no --threshold=85 "$work/$variant.cg" > "$work/$variant-profile.txt"
done
PLENTY_CACHE=off target/release/plenty --dump clif-opt,asm --dump-filter count_frames benchmarks/game/k-nucleotide/plenty/knucleotide.plenty -o "$work/count-frames"
