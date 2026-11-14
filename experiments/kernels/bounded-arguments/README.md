# Bounded arguments and direct extraction (issue #972)

`kernels.plenty` isolates spectral-norm's matrix helpers and multiplication
loops. `spectral_norm.plenty` is the measured bounded candidate, kept here
because it does **not** meet the individual-loop nonregression gate. The
benchmark in `benchmarks/game/spectral-norm/plenty/` remains unchanged.

Measurements use optimized native compilation on an AMD Ryzen 7 7840HS,
x86-64 Linux, GCC 16.2.1. Baseline compiler: `caf7c247`; after: this PR.
`runtime-results.json` contains nine interleaved samples per executable,
CPU 3, input 2000. `instructions:u` excludes kernel work; CPU time is
`task-clock:u`. Every executable prints `1.274224152`.

## Inner loops

Count with `python3 experiments/kernels/loops.py < asm.txt`.

| Source / build | A × u | Aᵀ × u |
| --- | ---: | ---: |
| C (`gcc -O2`, `kernels.c`) | 14 | 14 |
| Existing unchecked benchmark, before | 16 | 18 |
| Bounded candidate, before | 22 | 22 |
| Bounded candidate, after | 17 | 17 |
| Existing unchecked benchmark, x86-64-v3 | 15 | 17 |
| Bounded candidate, after, x86-64-v3 | 16 | 16 |

The bounded candidate removes seven instructions per pass: four stack stores,
two loads, and one zeroing instruction for the second argument's high word.
The new allocation introduces a counter move and split loop latch (conditional
exit plus unconditional backward jump), adding two: 22 − 7 + 2 = 17.
There are no hot bounds/overflow checks, calls, negative-index joins or header
reloads before or after; output bounds checks remain outside the inner loop.
The three remaining instructions against C are the separate floating load
(C folds it into `mulsd`), the counter move and the extra latch jump. C's
counter strength reduction rearranges arithmetic without changing its count.
The loop layout is related to issue #977; the separate floating-load folding
gap is recorded in issue #980. No backend fork is changed.

The bounded candidate is whole-program neutral by instruction count, but
A's individual loop grows 16 → 17 against the existing benchmark. The
benchmark rewrite therefore remains undelivered.

## Cause and IR variants

The checker already knows `Bounded` is one private integer field. Its class
calling convention nevertheless used I128, and `.get()` borrowed by address.
Inlining leaves a 32-byte frame with I128 stores and I64 loads; Cranelift does
not forward those mixed-width accesses. The fix uses the storage's native
integer convention and lowers only compiler-known numeric `.get()` operations
to direct field reads, retaining their shared-loan checks. Runtime aggregate
slots and ordinary user-class ABI stay unchanged.

Run the existing CLIF harness on `variants.clif`; count through the first
`retq` (the harness also disassembles constant-pool bytes).

| Variant | IR | Helper instructions, including frame/return |
| --- | --- | ---: |
| a | Original I128 parameters, stores and borrowed field loads | 23 |
| b | I128 parameters, direct low-field extraction, no borrowing slot | 14 |
| c | I64 parameters, direct field values | 14 |

Variant b isolates the address-taking cause. Variant c also removes unused
argument words from the native convention; in an isolated two-argument helper
this changes register assignment, not the instruction count. The actual
optimized helper has no stack slot, integer loads/stores, overflow checks or
calls. Tests cover all eight integer widths, signed extrema, full `u64`,
generic/indirect calls, fields, lists, references, `Result[Option[T], E]`,
fast compilation, alias diagnostics and unchanged ordinary-class ABI.

## Whole program

| Executable | Median instructions | Median CPU ms | CPU range ms |
| --- | ---: | ---: | ---: |
| Existing unchecked benchmark | 2,724,495,099 | 162.05 | 161.26–171.77 |
| Bounded candidate, before | 3,524,735,271 | 163.53 | 162.31–173.49 |
| Bounded candidate, after | 2,724,415,175 | 160.58 | 157.34–162.29 |
| Existing benchmark, x86-64-v3 | 2,564,495,383 | 161.13 | 159.14–161.47 |
| Bounded candidate, after, x86-64-v3 | 2,564,415,550 | 160.44 | 158.44–162.60 |
| C reference benchmark | 2,401,554,206 | 165.57 | 163.53–174.43 |

CPU ranges overlap; these results support no whole-program regression, not a
reliable speedup claim. Performance-counter counts are the primary evidence.

## Reproduction

Build each compiler separately with `cargo build --locked --release --bin plenty`.
For each compiler, compile both the unchanged benchmark and this candidate
with `PLENTY_CACHE=off plenty --release --compile SOURCE -o EXECUTABLE`.
Add `--cpu x86-64-v3` for the optional-ISA comparison. Dump kernels using
`plenty --release --dump asm,clif-opt kernels.plenty -o DUMP_DIRECTORY`.
Compile C with `gcc -O2 -c kernels.c`; count its `objdump -d --no-show-raw-insn`
output. The whole-program C reference is
`benchmarks/game/spectral-norm/reference/spectralnorm.clang-8.c`, built with
`gcc -O2 SOURCE -lm -o EXECUTABLE`.

Build the harness with
`cargo build --locked --release --manifest-path experiments/kernels/harness/Cargo.toml`.
Run `plenty-kernel-harness experiments/kernels/bounded-arguments/variants.clif`.
Use `perf stat -x, -e instructions:u,task-clock taskset -c 3 EXECUTABLE 2000`,
rotating executable order between rounds. The raw runtime samples preserve
the output and each counter reading.

## Compile time

Caches disabled, CPU 3. Three samples per build; instruction counts include
one warmup plus ten AOT repetitions for `compile_bench 1000 10 --workload aot`.
The harness sources are identical and separately built against each compiler.
Larger-program probes compile once per sample with `--compile`. Raw readings
are in `compilebench-results.json` and `compile-results.json`.

| Probe | Before median (range), M instructions | After median (range), M instructions | Change |
| --- | ---: | ---: | ---: |
| 1000-unit AOT harness | 128,716.397 (128,716.056–128,717.577) | 128,771.330 (128,770.258–128,775.196) | +0.043% |
| Original spectral-norm | 382.761 (382.741–382.799) | 383.977 (383.961–383.978) | +0.318% |
| Fasta | 391.113 (391.103–391.117) | 392.249 (392.228–392.267) | +0.291% |
| Bounded spectral-norm | 384.385 (384.365–384.392) | 384.848 (384.846–384.866) | +0.121% |

The compile-time increase is small but measurable; native-wrapper metadata is
consulted on additional type queries. The change trades this cost for removing
the generated accessor frame and spill/reload instructions.
