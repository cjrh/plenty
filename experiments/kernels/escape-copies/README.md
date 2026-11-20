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

The investigation kept the idiomatic benchmark as a fixed compiler regression
input. The [backend candidate](#backend-candidate) uses the same source to
check the imaginary multiply and real-value backedge.

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

## Backend candidate

The [local Cranelift patch](sse2-recurrences.patch) implements the operand
choice and real-value reuse on optimized SSE2 with the backtracking allocator.
It recognizes natural loop backedges whose scalar updates feed their own block
parameters. The imaginary multiply can destroy its right operand when that
operand is a header parameter with one use. The real square can destroy the
register already used for the real update. The backend delays the square past
independent arithmetic, then saves the unsquared value through a block argument
that can coalesce with the next iteration's parameter. This runs after
constant-phi cleanup, which would otherwise remove the single-predecessor
argument.

The multiply and square must run in the defining header or in its direct
latch with only terminating alternatives. This rules out a multiply that uses
the same outer parameter on every pass through an inner loop. The split tail
inherits the cold flag. Candidate discovery indexes each backedge's arguments
once and searches for at most one successful square move per block.

The rewrite preserves operation grouping and moves the square only across
independent arithmetic within its original block. Commuting the multiply can
change an arithmetic NaN's unspecified payload. AVX and unoptimized builds
bypass the rewrite.

Measured from Plenty `f8c061b2` with Cranelift `94ed85d10c`, compared with
the same compiler built against the local backend commit `2986d9c583`.

| Kernel or loop | Locked SSE2 | Candidate SSE2 | GCC SSE2 |
|---|---:|---:|---:|
| Isolated escape kernel | 20 | 16 | 15 |
| Benchmark `in_set` | 20 | 16 | |
| Inlined escape loop in `render_row` | 20 | 16 | |
| Scalar register copies in each `escape` pass | 6 | 3 | 3 |

The three backedge copies disappear. The three remaining copies preserve the
unsquared real and imaginary values and form their squared magnitude sum.
Both Plenty builds perform eight floating-point operations, a comparison, a
countdown subtraction, and two conditional branches. The candidate also drops
the unconditional backedge jump because the edge has no copies to place.
Its countdown still uses a separate test, which accounts for the remaining
one-instruction difference from GCC 16.2.1.

All 12 retained CLIF variants reach 16 instructions on the candidate. The C
oracle passes 480,000 comparisons on each of the SSE2 and AVX harness builds,
including signed zeros, subnormals, infinities and NaNs. The AVX machine-code
bytes match the locked backend. Another 480,000 comparisons cover the current
benchmark's bottom-tested form and its operand and scheduling variants.
The same variant comparison in single precision passes 480,000 cases.

Mandelbrot at size 4000 retires a median of **6,794.110 million instructions**
with the candidate, compared with **8,292.162 million** with the locked backend
and **6,289.015 million** with the C reference. The reduction is **18.07%**.
All outputs match the reference at sizes 1, 7, 8, 9, 200 and 4000, including
the checked-in size-200 bitmap. The 14 Benchmarks Game variants produce
identical output across three runs each. The other variants' median instruction
counts shift by at most 0.085%, within the spread observed in the hash workloads.

Uncached compilation retired the following instruction counts in three
alternating runs. Counts are in millions, with the sample range in parentheses.

| Workload | Locked compiler | Candidate compiler |
|---|---:|---:|
| `compile_bench 1000 10 --workload aot` | 129,914.834 (129,913.704–129,915.220) | 128,977.633 (128,974.661–128,978.115) |
| Spectral norm | 388.761 (388.750–388.776) | 388.483 (388.468–388.509) |
| Bounded spectral norm | 388.740 (388.732–388.753) | 388.492 (388.481–388.515) |
| Fasta | 396.914 (396.880–396.915) | 396.710 (396.697–396.771) |

The [raw instruction counts and output hashes](backend-measurements.json)
cover the runtime and compilation comparisons. Cranelift's registry allocator
passes all 211 x64 filetests and 19 x64-only runtime tests. The new regression
fails on the locked backend and passes with the candidate. The nested-loop
and cold-layout regressions fail with the original patch and pass after the
revision. Plenty's pinned allocator produces the same 15 assembly-output
mismatches and identical diff hunks as the locked backend.

A 4,096-accumulator loop whose backedge carries only the squares exposed
the repeated scan. The harness retires 22,457.074 million instructions
with the revision, down from 22,912.543 million with the original
candidate, while emitting identical machine code. These counts include
the harness and allocator. The membership lookup now takes one pass
over the edge arguments.

### Reproduce the candidate

Apply the patch in an isolated Cranelift checkout at `94ed85d10c`.
Build Plenty and the CLIF harness with path overrides pointing at that
checkout and keep the compiled-function cache off. Use the
[repository fork procedure](../../../book/src/development/workflow.md#cranelift-and-regalloc2-forks)
for the Plenty override. The harness overrides `cranelift-codegen`,
`cranelift-control` and `cranelift-reader` under the same patch table.
Run the commands in [Reproduction](#reproduction) with the patched tools
and then repeat with the locked tools. For the unchanged benchmark, dump
`in_set` and `render_row` separately with `--dump-filter` and pass
`asm.txt` to `loops.py -v`. The new Cranelift filetest checks the complete
SSE2 sequence with the register-allocation checker.

The candidate is available locally. It needs fork publication and lockfile
updates before release builds use it.
