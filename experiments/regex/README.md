# B48 regex acceleration (issues #742 and #752)

This change keeps the regex-redux source, patterns and single-threaded counting
unchanged. Prefix filtering, a bounded lazy DFA and a one-pass engine live in
`stdlib/std/regex.plenty`; captures and unsupported cases retain the Pike VM.
No Rust regex implementation is linked into generated Plenty programs.

Measurements on 2026-11-05, x86-64 AMD Ryzen 7 7840HS, optimized builds.
Baseline checkout: `fe11d0a2` (`main` at task start), using its existing release
compiler. Candidate: this branch, rebuilt with
`CCACHE_DISABLE=1 cargo build --locked --release --bin plenty`.
The C compiler is GCC 16.2.1. The checked-in Rust reference pins regex 1.12.3.

## Whole-program instructions

`perf stat -e instructions:u` includes input, regex compilation, all searches,
replacement allocation, output and process startup. The full input is generated
by the benchmark's FASTA reference with argument 5,000,000. Every output was
compared byte-for-byte with the baseline and Rust reference.

| Build | Input size argument | Retired user instructions |
|---|---:|---:|
| Baseline Pike VM | 50,000 | 12,892,945,967 |
| Prefix filter prototype | 50,000 | 2,477,286,988 |
| Filter + lazy DFA prototype | 50,000 | 1,969,653,420 |
| Baseline Pike VM | 5,000,000 | 1,289,059,356,781 |
| Final accelerators + cursor validation, run 1 | 5,000,000 | 114,580,307,324 |
| Final, run 2 | 5,000,000 | 114,580,362,528 |
| Final, run 3 | 5,000,000 | 114,580,353,372 |
| Rust reference, nine counting threads | 5,000,000 | 8,411,013,055 |

The full workload uses **11.25 times fewer instructions** (91.11% reduction).
The three candidate counts span 55,204 instructions (0.000049%). It remains
13.62 times the reference's total instructions, including all nine threads.
These are instruction ratios, not wall-clock speedups. The stored
`benchmarks/game/baseline.json` reports 59.7 s Plenty versus 0.74 s Rust;
wall-clock observations on this busy shared machine are not comparable to that
historical session.

## Compilation cost

With `PLENTY_CACHE=off`, compiling the unchanged regex-redux executable and its
reachable std code costs:

| Compiler | Three instruction counts |
|---|---|
| Baseline | 869,734,938; 875,878,684; 870,998,590 |
| Candidate | 1,024,507,345; 1,033,299,330; 1,024,683,349 |

The median increases 17.64% because the regex module now contains more code.
This is application compilation, not regex compilation; regex compilation is
included in the whole-program measurements above. There are no compiler or
Cranelift changes in this PR. Generic `compile_bench`/CLIF-variant comparisons
were not used to select these standard-library algorithms.

## Prefix scan kernel

`prefix.plenty` and `prefix.c` implement the same bit-parallel prefix scan over
borrowed bytes with an explicit work budget. They do not model the production
text-byte runtime call. C relies on the listed preconditions. The Plenty probe
keeps bounds checks; its `--unchecked assume` arithmetic matches the standard
library's internal arithmetic policy. The ordinary trapping build also remains
valid and prints `(2, 4)`.

| Build | Hot-path instructions per byte |
|---|---:|
| C, GCC 16.2.1 `-O2` | 11 |
| Plenty, optimized `--unchecked assume` | 23 |
| Plenty, optimized default trapping arithmetic | 29 |

Attribution of the 12 extra instructions in the assume build:

| Cause | Extra instructions |
|---|---:|
| Two bounds checks (`cmp`, conditional jump) | 4 |
| Register moves | 2 |
| Redundant byte extension | 1 |
| Separate address construction | 2 |
| Separate shift and OR versus one LEA | 1 |
| Separate mask load versus memory AND | 1 |
| Loop-back jump | 1 |

The trapping build adds six overflow-check instructions. No header reload,
spill or helper call appears in this kernel's hot loop. The production text
scanner additionally calls the native byte reader; a pre-one-pass profile put
12.23% of cycles in that reader. These remaining costs are issue #753. No
compiler checks were removed, and no pinned backend was modified. IR variants
are deferred to that compiler/bulk-scan investigation.

## Reproduce

From this worktree:

```sh
CCACHE_DISABLE=1 cargo build --locked --release --bin plenty
PLENTY_CACHE=off CCACHE_DISABLE=1 python3 -B benchmarks/game/bench.py \
  --work /tmp/regex-b48 run --mode full --repeats 3 regex-redux
perf stat -e instructions:u /tmp/regex-b48/bin/regex-redux.plenty-idiomatic \
  < /tmp/regex-b48/inputs/fasta-5000000.txt > /tmp/regex-b48-plenty.out
perf stat -e instructions:u /tmp/regex-b48/bin/regex-redux.reference \
  < /tmp/regex-b48/inputs/fasta-5000000.txt > /tmp/regex-b48-rust.out
cmp /tmp/regex-b48-plenty.out /tmp/regex-b48-rust.out
```

For repeated instruction measurements, reopen stdin for each invocation; do not
use `perf stat -r` with a single redirected input file, which leaves later runs
at EOF. Keep the baseline executable before switching builds.

```sh
gcc -O2 -fno-tree-loop-distribute-patterns -c experiments/regex/prefix.c -o /tmp/prefix.o
objdump -d --no-show-raw-insn /tmp/prefix.o | python3 experiments/kernels/loops.py
PLENTY_CACHE=off target/release/plenty --unchecked assume --dump asm \
  experiments/regex/prefix.plenty -o /tmp/prefix-dump
```

Normalize Plenty's assembly with the recipe in `experiments/kernels/README.md`,
then pass it to `experiments/kernels/loops.py`. There is no old version of this
new filter kernel: the old engine's work is measured by the whole-program
baseline. The largest improvement comes from changing the search algorithm,
not from changing the generated instructions for a VM operation.

## Bounds and remaining scope

- Prefix extraction: at most 256 NFA instructions and eight ASCII positions.
- Lazy DFA: at most 62 NFA instructions, 64 cached subsets and 128 byte columns;
  existence queries and fixed-length match offsets, without assertions.
- One-pass: at most 62 instructions, disjoint preferred ASCII transitions;
  only whole-match offsets. Unicode or excessive retry work resumes the VM.
- Subgroup captures use the VM. Optional engines preserve fallible allocation,
  work budgets, and memory headroom; no new public declarations are needed.
- Issue #752 binds cursor compatibility to the exact pattern and search budgets,
  preventing stale transitions when equal-size programs differ.
- B48 retains broader Unicode/assertion acceleration and Unicode feature work.
  Native/bulk literal scanning and the remaining generated-code costs are #753.
