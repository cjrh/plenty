# Shared-window lowering (issue #950)

These kernels compare computed range slices with the shared-window loop plan.
The Plenty, C and Rust moving-sum programs preserve ordered `f64` accumulation
(no fast math or contraction). Each uses 1,024 elements, width four and 100,000
repetitions; each prints `3063000000.0`. Change the Plenty main call from
`windows(&xs[:], 4)` to `ranges(&xs[:], 4)?` for the baseline.

Build Plenty with `cargo build --locked --release --bin plenty`, then compile
both Plenty variants with `target/release/plenty --compile SOURCE -o OUTPUT`.
Build C with `gcc -O2 -ffp-contract=off moving-sum.c -o moving-c`, and Rust with
`rustc -O moving-sum.rs -o moving-rust`. Dump each Plenty kernel with the
compiler's `--dump asm --dump-filter FUNCTION` options; `experiments/kernels/loops.py` can
extract inner loops. Use the same compiler for both Plenty variants.

The second workload is `benchmarks/game/k-nucleotide`: compare the range loop
from the parent commit with the windows loop in this change, compiled with the
same compiler. Generate input with the benchmark's Rust FASTA implementation
at size 250,000 (2.54 MB total, 1.25 million bytes in the selected sequence).
Compile its Rust reference as the external comparison. Check complete stdout
for equality before comparing costs. The parallel Plenty variant is a
compilation/correctness check, not part of these serial-loop measurements.

## Recorded comparison

`measurements.json` contains five interleaved rounds on an AMD Ryzen 7 7840HS,
Linux x86-64, pinned to logical CPU 0. Instructions are whole-process retired
user instructions from `perf stat -e instructions:u,task-clock`; user CPU comes
from child resource usage and includes the small measurement launcher cost.
The Rust k-nucleotide reference creates seven workers, all constrained to the
same CPU for this comparison. These are not scaling measurements.

| Program | Median instructions | Median user CPU (s) |
| --- | ---: | ---: |
| Moving sum, range slices | 3,679,534,864 | 0.312025 |
| Moving sum, windows | 3,576,434,821 | 0.290556 |
| Moving sum, C | 2,350,467,718 | 0.293000 |
| Moving sum, Rust | 2,759,833,678 | 0.297346 |
| k-nucleotide, range slices | 2,333,470,611 | 0.488571 |
| k-nucleotide, windows | 2,212,111,394 | 0.271159 |
| k-nucleotide, Rust | 3,812,520,583 | 0.692199 |

Instruction counts fall about 2.8% and 5.2% respectively. CPU samples overlap
widely on this shared host: k-nucleotide range samples span 0.227–0.498 s and
window samples 0.224–0.500 s. The median CPU difference is not evidence of a
reliable throughput gain. Instruction counts are the stronger result here.

## Assembly attribution

Both Plenty moving-sum versions have a five-instruction inner float loop.
The improvement is outside that loop: range slicing performs checked integer
end arithmetic and clamps the selected bounds for each window. The window
plan proves the range once from remaining length and constructs its descriptor
without a call or trap. C still has fewer instructions: its float loop combines
memory operands and unrolls pairs, while Plenty retains separate loads and
adds plus its counter. No claim of C instruction parity is made.

In k-nucleotide, the old loop calls `plenty_byte_view` and clamps range bounds
for every dictionary key. The new plan obtains the byte view before the loop
and forms each key view directly from pointer and count. Dictionary lookup,
hashing and checked counter increments retain their existing behavior. This
change does not optimize those operations or unchecked arbitrary indexing.
