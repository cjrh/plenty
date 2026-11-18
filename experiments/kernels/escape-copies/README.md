# Scalar copies in the Mandelbrot escape loop (issue #978)

The fixed benchmark and standalone `escape` kernel both use 20 instructions
per steady-state pass on the x86-64 baseline. The scalar C kernel uses 15.
This investigation uses Plenty 0.8.32 at `294c0a9c`, the pinned Cranelift
`d27f16528a`, regalloc2 `b2dc24bb70`, and GCC 16.2.1 with SSE2 and floating
contraction disabled. The separate [counted escape-latch experiment](../escape-latch/README.md)
accounts for the countdown test and back jump.

| Instruction | GCC SSE2 | Plenty SSE2 | Commuted product SSE2 | Plenty x86-64-v3 |
|---|---:|---:|---:|---:|
| Floating arithmetic | 8 | 8 | 8 | 8 |
| Floating compare | 1 | 1 | 1 | 1 |
| Countdown subtraction | 1 | 1 | 1 | 1 |
| Conditional branches | 2 | 2 | 2 | 2 |
| Separate countdown test | 0 | 1 | 1 | 1 |
| Unconditional back jump | 0 | 1 | 1 | 1 |
| Scalar register copies | 3 | 6 | 5 | 1 |
| Total | 15 | 20 | 19 | 15 |

The original SSE2 loop copies a value for the magnitude sum and each square
because their two-address instructions would otherwise overwrite a value the
recurrence still needs. Three further copies reconcile the registers for
`zi`, `zr`, and `tr` across the backedge. GCC reuses the incoming `zi` register
for the imaginary product and its sum. For the real update, it preserves the
new `zr` in its loop register and squares in the existing `tr` register. This
uses one copy for two purposes: preserving the unsquared value and placing it
where the next iteration reads it.

Changing `2.0 * zr * zi` to `zi * (2.0 * zr)` in an experimental copy selects
the incoming `zi` as the first operand of the second multiply. SSE2 uses that
operand's register as the result, so the add and the next iteration can keep
the updated `zi` there. The loop drops one backedge copy. It keeps the two
multiplications in the same grouping, avoids reassociation and still checks
the same magnitude. The only observed result is a Boolean, so any NaN payload
selection within the product is immaterial to it.

The remaining real pair needs a better way to place both the square and its
unsquared operand. On AVX, three-address operations remove the need for
operand-preservation copies and permit more coalescing. The one remaining
copy carries the new `zr`; the separate latch overhead remains. The operand
order result establishes a lowering opportunity, while the allocator probe
below shows why merely moving the backedge merges earlier is insufficient.

## IR variants

`build_probes.py` derives the variants from the original and rotated CLIF in
the counted-latch experiment. As an allocator control, the retained
`rejected-loop-edge-priority.patch` tries edges originating in loops before
two-address reuse constraints, followed by the regular block-parameter pass.
It did not improve the original and increased the rotated and commuted path
from 19 to 21 instructions. It is a rejected probe and was never applied to
the production allocator.

| IR change | SSE2 | AVX | Early edge merge probe (SSE2) |
|---|---:|---:|---:|
| Original | 20 | 15 | 20 |
| Commute second imaginary multiply | 19 | 15 | 19 |
| Commute magnitude sum | 20 | 15 | 20 |
| Commute multiply and sum | 19 | 15 | 19 |
| Rotate escape check | 20 | 15 | 20 |
| Rotate and commute multiply | 19 | 15 | 21 |
| Rotate and commute sum | 20 | 15 | 20 |
| Rotate and commute both | 19 | 15 | 21 |
| Pass real and imaginary state before squares | 21 | 15 | 20 |
| Reorder parameters and commute multiply | 20 | 15 | 19 |
| Separate imaginary arithmetic into a preceding block | 20 | 15 | 20 |
| Imaginary arithmetic first and commute multiply | 19 | 15 | 19 |

All three builds passed 480,000 C/CLIF comparisons each. Cases include signed
zeros, normal and subnormal numbers, infinities, quiet and signaling NaNs,
random bit patterns and grid values. The checker compares escape decisions.

## Whole program

Three alternating runs per profile, pinned to CPU 3, at size 4000. SSE2 and
x86-64-v3 use the same compiler. The SSE2 C reference is the checked-in
Benchmarks Game source built using the same GCC flags as the scalar kernel.
Its whole-program instruction count also reflects a different PBM output path.

| Build | Median instructions (millions) | Task-clock median (range), ms |
|---|---:|---:|
| Plenty baseline SSE2 | 8,202.034 | 836.25 (827.86–840.81) |
| Plenty commuted product SSE2 | 7,827.521 | 830.15 (821.78–834.13) |
| Plenty baseline x86-64-v3 | 6,265.470 | 825.47 (825.25–856.60) |
| GCC SSE2 | 6,289.012 | 846.56 (844.12–847.76) |

The single-copy removal cuts program instructions by 4.57%. The Plenty timings
overlap. All 12 size-4000 PBMs match byte for byte. Sizes 1, 7, 8, 9 and 200
also agree across all four builds, including the tracked expected output at
size 200. Uncached compilation of the same sources takes a median of 329.363M
instructions for the default profile (329.354M–329.385M), 329.351M for the
commuted source (329.335M–329.364M), and 329.882M for x86-64-v3
(329.866M–329.888M). Raw samples are in `measurements.json`.

This work retains the idiomatic benchmark as a fixed compiler regression
input. Issue #978 remains open for compiler or backend promotion of the
imaginary multiply's register choice and for the two remaining real-value
backedge copies. No generic allocator or compiler change is proposed.

## Reproduction

Use the pinned release compiler and CLIF harness from the repository root.

```sh
scratch=$(mktemp -d)
cargo build --locked --release --bin plenty
cargo build --locked --release --manifest-path experiments/kernels/harness/Cargo.toml \
    --target-dir target/kernel-harness
python3 -B experiments/kernels/escape-copies/build_probes.py "$scratch/variants.clif"
gcc -O2 -ffp-contract=off -Wall -Wextra -Werror \
    experiments/kernels/counted-latch/verify-escape.c -o "$scratch/verify"

PLENTY_KERNEL_CODE_DIR="$scratch/sse2" \
    target/kernel-harness/release/plenty-kernel-harness "$scratch/variants.clif" > "$scratch/sse2.asm"
python3 -B experiments/kernels/loops.py -v < "$scratch/sse2.asm"
"$scratch/verify" "$scratch/sse2"
PLENTY_KERNEL_AVX=1 PLENTY_KERNEL_CODE_DIR="$scratch/avx" \
    target/kernel-harness/release/plenty-kernel-harness "$scratch/variants.clif" > "$scratch/avx.asm"
python3 -B experiments/kernels/loops.py -v < "$scratch/avx.asm"
"$scratch/verify" "$scratch/avx"

gcc -O2 -march=x86-64 -ffp-contract=off -fno-tree-loop-distribute-patterns \
    -c experiments/kernels/unused-range/kernels.c -o "$scratch/c.o"
objdump -d --no-show-raw-insn "$scratch/c.o" | python3 -B experiments/kernels/loops.py -v
gcc -O2 -march=x86-64 -ffp-contract=off -fno-tree-loop-distribute-patterns \
    benchmarks/game/mandelbrot/reference/mandelbrot.clang-8.c -o "$scratch/mandelbrot-c"

source=benchmarks/game/mandelbrot/plenty/mandelbrot.plenty
sed 's/2\.0 \* zr \* zi/zi * (2.0 * zr)/' "$source" > "$scratch/commuted.plenty"
target/release/plenty --no-cache --compile "$source" -o "$scratch/mandelbrot-sse2"
target/release/plenty --no-cache --compile "$scratch/commuted.plenty" -o "$scratch/mandelbrot-product"
target/release/plenty --no-cache --cpu x86-64-v3 --compile "$source" -o "$scratch/mandelbrot-v3"
for name in sse2 product v3 c; do
    perf stat -e instructions:u,task-clock taskset -c 3 "$scratch/mandelbrot-$name" 4000 > "$scratch/$name.pbm"
    cmp "$scratch/sse2.pbm" "$scratch/$name.pbm"
done
perf stat -e instructions:u,task-clock taskset -c 3 target/release/plenty \
    --no-cache --compile "$scratch/commuted.plenty" -o "$scratch/compile-probe"
```

For the allocator control, apply the retained patch to an isolated copy of
regalloc2 at the pin above and build a separate copy of the harness with its
`[patch.crates-io]` `regalloc2` entry pointing to that copy. Reuse the SSE2
harness and C checker commands with the resulting executable.
