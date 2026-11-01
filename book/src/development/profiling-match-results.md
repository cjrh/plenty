# Match dispatch measurements

Historical dispatch experiments; see [Profiling the compiler](profiling.md) for the profiling guide and the [emission reference](../design/toolchain/02-architecture/emission.md#match-dispatch) for current behavior.

## Dense-match dispatch

The 2026-10-22 B19 experiment at `9c994af0` measures dense integer and enum
dispatch with 4, 16, 64 and 256 arms against the then-production comparison chains.
A temporary Cranelift `Switch` patch produces `br_table`; the patch is preserved
as the original experiment. The [production policy](#production-policy) addresses
issue #395. Reproduction, samples and generated-fixture methodology
are in [`examples/dense_match/`](https://github.com/cjrh/plenty/blob/main/examples/dense_match/README.md).

On a Ryzen 7 7840HS, x86-64 Linux, Cranelift 0.131.1, pinned to one CPU, median
wall times over seven shuffled rounds for two million dispatches were:

| Arms | Integer uniform: chain / switch | Enum uniform: chain / switch | Integer 90%-last: chain / switch |
|---|---:|---:|---:|
| 4 | 13.1 / 15.7 ms | 23.0 / 26.9 ms | 8.5 / 9.9 ms |
| 16 | 16.7 / 18.4 ms | 27.2 / 29.2 ms | 11.3 / 10.5 ms |
| 64 | 23.1 / 18.7 ms | 33.9 / 30.8 ms | 22.6 / 10.0 ms |
| 256 | 48.1 / 19.4 ms | 84.0 / 31.2 ms | 68.0 / 11.0 ms |

The same-session baseline and prototype runs interleave. Times include process
startup, all-arm correctness checks, the loop and output; enums also include
borrowed list indexing. They are not pure dispatch latency or integer-versus-enum
ABI comparisons. A Python oracle checks every sequence's checksum; integer
fallbacks include the first out-of-range value and `u64.MAX`.

The 256-arm enum 90%-last case fell from 111.4 to 14.6 ms. First-arm-heavy cases
show why distribution matters: switches regressed at 4/16/64 arms for both
integers and enums; at 256 they improved from 11.7 to 9.2 ms and 18.2 to 14.6 ms.
A balanced Plenty integer decision tree was slower than chains for uniform
4/16/64-arm inputs (14.7/22.9/30.2 ms), and slower than the switch at 256
(37.9 ms). Neither balanced trees nor switches should replace every chain.

At 256 arms, isolated dispatch size fell from 5,515 to 3,656 bytes for integers
and 7,763 to 3,688 for enums, including table data/padding. At four arms it grew
from 91 to 128 and 131 to 152 bytes. Optimized CLIF instruction counts at 256
fell from 1,284 to 521 and 1,549 to 534. Cold compile/link medians for the full
uniform fixtures were 69/65 ms (integer chain/switch) and 95/99 ms (enum);
these include validation/setup code and linking, with only three samples in
separate batches. They establish no reliable compile-time gain.

### Production policy

The production lowerer now selects a jump table for at least 256 distinct
contiguous integer/tag values, in any source order (issue #395). Smaller and
sparse sets retain chains. The threshold deliberately leaves the mixed 64-arm
results alone; it is not claimed to be optimal for every CPU or distribution.
Regression checks cover all eight integer widths, negative/offset ranges,
signed extrema, unsigned maxima, distant 64-bit fallbacks, reversed cases,
alternatives, nested payload ordering, owned/borrowed enum payloads and tail
exits. Exhaustive enum matching retains the invalid-tag trap. The
[emission contract](../design/toolchain/02-architecture/emission.md#match-dispatch)
describes index normalization and bounded table storage.

The harness also offers `--kinds work`: each arm computes a different wrapping
multiply/add over a runtime state argument. This avoids drawing the entire
conclusion from constant-return arms, while remaining a synthetic workload.
It does not establish a benefit for any existing application. No source syntax
or tutorial idiom changes follow from this optimization.

A follow-up paired run against the chain compiler at `70aa0a3e` validated the
production policy on the same host and CPU affinity, using two million
dispatches and seven shuffled rounds (672 checked timed executions):

| 256-case workload | Uniform chain / table | 90%-first chain / table | 90%-last chain / table |
|---|---:|---:|---:|
| Integer | 55.6 / 21.0 ms | 12.2 / 10.5 ms | 79.7 / 12.1 ms |
| Enum | 100.1 / 35.1 ms | 19.7 / 15.6 ms | 127.7 / 17.8 ms |
| Runtime multiply/add | 57.6 / 22.1 ms | 13.5 / 10.5 ms | 82.1 / 11.8 ms |

Isolated dispatch bytes, including table data/padding, fell from 5,515 to 3,646
for integers, 7,763 to 3,674 for enums and 7,623 to 5,754 for runtime arithmetic.
All 26 optimized-IR/assembly dump files for the smaller matches and unchanged
tree controls are byte-identical between compilers.
Full samples, compiler/source hashes and dump indexes are preserved in
[`production-results.json`](https://github.com/cjrh/plenty/blob/main/examples/dense_match/production-results.json).
Cold build samples were collected in separate batches with other builds active
during the baseline batch; they establish **no compile-time gain**. The machine
was not reserved or frequency-locked, so small timing differences in unchanged
controls are noise rather than evidence for tuning smaller matches. This run
does not measure allocation or peak memory, nor justify a broader sparse-table
policy.

## String match dispatch

A `match` with at least 16 distinct `str` literals before its fallback calls
`plenty_str_match` once and branches through a `br_table` (issue #565). The
runtime binary-searches a read-only table of the literals, ordered by byte
length and then bytes; each entry maps to the first source arm with that
literal. Smaller matches keep the `plenty_str_eq` chain. The table stores
32-bit offsets followed by the literal bytes, so it needs no load-time
relocations in a PIE.

The 2026-10-30 measurements at `1756617d` used a Ryzen 7 7840HS, x86-64 Linux
and Cranelift 0.137, pinned to the eight physical cores. Each program matches
the first `n` sorted IANA zone names (a seeded sample below 597) and four
misses: a longer string, a shorter prefix, the empty string and an unknown
name. Inputs are split from one literal at run time, so no lookup compares
literal addresses. Times are medians and ranges of five shuffled rounds of
two million lookups, including process start:

| Arms | Uniform: chain / table | First arm only: chain / table | `find` bytes: chain / table |
|---:|---:|---:|---:|
| 5 | 32.5 / 32.9 ms (chain kept) | 18.3 / 17.1 ms | 283 / 283 |
| 15 | 58.5 / 59.6 ms (chain kept) | 17.8 / 17.7 ms | 693 / 693 |
| 16 | 59.7 [59.4–60.5] / 49.2 [48.6–52.1] ms | 18.2 / 28.0 ms | 734 / 357 |
| 50 | 123.8 [123.0–125.0] / 65.1 [63.5–66.2] ms | 17.7 / 30.9 ms | 2,128 / 831 |
| 597 | 1,078.8 [1,077.8–1,087.6] / 110.6 [109.4–112.3] ms | 18.7 / 46.3 ms | 24,555 / 8,491 |

At 597 arms a uniform lookup falls from about 530 to 46 ns. The table data
adds 4(n + 1) bytes plus the literal bytes. Optimized CLIF instruction counts
fall from 115 to 55 at 16 arms and from 3,601 to 1,217 at 597. Cold compile
times were within noise of each other (66–71 ms at 597 arms).

A run that forced the table for every size found the chain faster for uniform
inputs below about 12 arms (5 arms: 33.8 / 38.1 ms), so 16 is where the table
clearly pays. When the first arm matches every input, the chain always wins,
by about 5 ns per lookup at 16 arms and 14 ns at 597: the chain stops after
one comparison, while the search always takes about log2(n) steps. Order arms
by frequency only in a small match; a large match no longer depends on order.
A same-length comparison by 8-byte big-endian words was slower than the
byte-slice comparison, which calls `memcmp`, and was not kept.
