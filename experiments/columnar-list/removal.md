# Ordered removal measurements

The initial `clist.remove(index)` implementation gathers an ordinary owned row,
then shifts each native-width column with an overlapping copy. `clear()` sets
the live length to zero; primitive rows need no per-field destruction. Both
retain capacity and allocate nothing, verified with forced allocation failure.

Run `python3 experiments/columnar-list/removal.py COMPILER --out OUTPUT --cpu 6`
with a compiler built **without** `runtime-checks`. The script generates Plenty
programs, compiles with `--release`, checks returned-row checksums, and records
all samples. Construction is outside the timer. Each timed batch performs 256
remove/append pairs at a fixed length, keeping capacity constant. Times include
row gathering, consuming the first field, and appending a replacement. These
are costs of that workload, not isolated `memmove` latency or a comparison
against ordinary lists. One warmup precedes seven retained samples.

Final measurement on 2026-10-25, AMD Ryzen 7 7840HS, Linux x86-64, CPU 6 affinity,
after this task's tests finished. No cargo, rustc or integration-test process
was running when sampled. The emitted programs use the existing baseline ISA.
The retained JSON includes all sample times and the compiler binary hash.

Median microseconds per remove/append pair (native row widths, all `i64` fields):

| Rows | Row bytes | Front | Middle | Last |
| ---: | ---: | ---: | ---: | ---: |
| 8,192 | 8 | 0.811 | 0.402 | 0.041 |
| 8,192 | 32 | 3.177 | 1.702 | 0.129 |
| 8,192 | 128 | 15.058 | 7.450 | 0.975 |
| 65,536 | 8 | 6.133 | 3.027 | 0.041 |
| 65,536 | 32 | 26.728 | 13.976 | 0.131 |
| 65,536 | 128 | 107.737 | 56.106 | 1.014 |

The result supports a simple cost model: ordered front/middle removal scales
with the bytes shifted; last-row removal avoids that movement but still pays
for gathering and handling every field. The widest front case ranged from
106.508 to 121.929 microseconds. Treat these as illustrative local costs,
not portable throughput guarantees. Repeated front removal is quadratic;
chunks, tombstones, batch compaction and specialized native removal lowering
are outside this change. No microbenchmark is needed to establish clear's
constant-time implementation: it writes only the header length.
