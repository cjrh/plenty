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
keeps bounds checks; its `--unchecked assume` arithmetic is a diagnostic comparison under the
listed preconditions. Default builds retain arithmetic traps, including in std. The ordinary trapping build also remains
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


## Issue #753: borrowed scans and DFA restart filtering

Baseline is merged `main` at `95d4c667`; candidate is the issue #753 branch.
Both were rebuilt with `cargo build --locked --release --bin plenty` and use
optimized default builds, including arithmetic traps. The unchanged full
regex-redux input, patterns, single-threaded counting and output comparison
are the same as above. Do not reuse an old compiler executable: it embeds its
standard-library sources. Preserve each rebuilt compiler and benchmark binary.

| Full regex-redux | Run 1 | Run 2 | Run 3 |
|---|---:|---:|---:|
| Merged baseline | 114,121,635,608 | 114,121,635,832 | 114,121,632,508 |
| Borrowed scans + filtered DFA + indexed accounting | 63,497,711,754 | 63,497,724,772 | 63,497,712,450 |

The median drops **44.36% (1.80× fewer instructions)**, with a 13,018-instruction
candidate spread. Every output equals both the baseline and pinned Rust
reference byte-for-byte. The earlier validated #742 executable cross-checks
at 114,065,898,573 instructions; the rebuilt baseline above includes intervening
main changes. Intermediate prototypes measured 90,799,814,586 with borrowed
reads alone and 68,553,702,455 with DFA restart filtering and slice iteration.
These remain instruction comparisons, not elapsed-time speedups.

The compiler's argument-only `__regex_text_bytes` operation borrows a substring
and forwards its loan to the consuming byte-view parameter. Its conversion is
a representation-preserving retag: substring views already contain a byte
address and byte count. It cannot escape into a local binding, be returned,
or produce mutable bytes. Inline text, heap text, substrings and temporaries
use the existing view lifetime rules. No runtime ABI or dependency changes.
The matching engines and prefix extraction remain Plenty.

The lazy DFA uses the conservative prefix filter whenever its state returns
to its initial subset. The filter reads a borrowed byte slice directly. Its
iterator index counts consumed bytes; work and output positions are calculated
only on return. Budget exhaustion, empty input and counters near `i64.MAX` have
regression coverage. No compiler bounds/overflow-check rule changed.

### Kernels and IR shapes

Use the dump/normalization commands above for each kernel, with the default
arithmetic policy unless the column explicitly says `--unchecked assume`.
`text-before.plenty` preserves the old native-byte-read loop; `text-view.plenty`
passes the borrowed text view to the final kernel. `prefix.plenty` preserves
the original indexed byte-view kernel; the `-iter` and `-index` variants isolate
the two source/IR shape changes. All print `(2, 4)`.

| Kernel / IR shape | Default checked | Diagnostic assume |
|---|---:|---:|
| C `scan`, GCC 16.2.1 `-O2` | 11 | — |
| Original indexed byte view (`prefix.plenty`) | 29 | 23 |
| Slice iteration (`prefix-iter.plenty`) | 25 | 19 |
| Iterator index accounting (`prefix-index.plenty`) | 19 | 17 |
| Old text-native loop (`text-before.plenty`) | 61 + callees | 53 + callees |

Counts refer to the standalone `scan` body. Inlining can change registers and
counts: the final checked `main` loop is 17 instructions. The old text kernel's
53 excludes work in its native callees. This table must not be read as the
full matcher's per-byte cost. The final production `prefix_candidate` loop
measures 17 instructions with default checks (use `--dump-std --dump-filter
prefix_candidate` on regex-redux to inspect it).

Relative to the original 23-instruction assume kernel, slice iteration removes
the byte bounds check (2), one address construction and two register moves,
while adding an iterator counter increment: net −4. Index accounting removes
per-byte offset/work increments and their default arithmetic checks, but changes
register allocation. The final checked kernel's eight extras over C are a mask
bounds check (2), redundant byte extension (1), separate mask-address creation
(1), mask load (1), separate shift/or rather than LEA (1), loop-back jump (1),
and mask-length spill/reload (2), offset by one fewer induction update (−1).
The last accounting is specific to the dumped standalone function.

These variants reuse existing slice/iterator lowering. There is no hand-edited
CLIF replacement or new loop-lowering optimization: the only codegen addition
retags an already validated view without emitting instructions. Further check
proofs and backend experiments belong to issue #758, together with selective
bulk multi-literal filtering. No pinned backend was patched.

### Application compilation

With `PLENTY_CACHE=off`, retired instructions for compiling the unchanged
regex-redux source (not compiling regex patterns at runtime):

| Compiler | Run 1 | Run 2 | Run 3 |
|---|---:|---:|---:|
| Merged baseline | 1,025,322,867 | 1,029,191,058 | 1,026,913,367 |
| Candidate | 1,038,055,465 | 1,036,895,478 | 1,040,810,769 |

The median increases 1.09%. The source gains a shared scan helper and the
borrowed-view entry path. Runtime regex compilation is already included in
whole-program measurements. B48 still retains Unicode/assertion acceleration
and Unicode feature expansion; issue #758 narrows the remaining scan work.


The general `compile_bench 1000 10 --workload aot` qualification, with
`PLENTY_CACHE=off` and its normal warm-up, retired 134,152,140,279;
134,161,518,172; and 134,153,425,719 instructions (spread 9,377,893).
These candidate-only runs establish a reproducible current result; the
before/after compilation comparison is the application table above.
