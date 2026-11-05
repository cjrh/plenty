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

## Issue #758: selective paths and bounded native batches

Measured on 2026-11-05 against local main `d951457d`, with freshly rebuilt
optimized compilers, default arithmetic checks, the unchanged full FASTA
5,000,000 input and single-threaded Plenty counting. The first attempted
baseline accidentally embedded a prototype while its build was running; those
numbers were discarded and the baseline rebuilt before these comparisons.

| Full regex-redux | Run 1 | Run 2 | Run 3 |
|---|---:|---:|---:|
| Main `d951457d` | 62,270,122,573 | 62,270,152,522 | 62,270,146,955 |
| Native literal batches + packed fallback | 55,179,085,028 | 55,179,068,896 | 55,179,113,686 |

Median retired instructions fall **11.39%**, or 1.13× fewer instructions.
Candidate spread is 44,790 instructions. All runs match the baseline and a
fresh build of the pinned Rust reference byte-for-byte. The Rust reference
uses 8,519,215,109 instructions with its nine counting threads pinned to CPUs
0–7; the
candidate remains about 6.48× its instruction count. No wall-clock claim.

### Selection and bounds

The existing position-wise filter admits combinations across alternatives.
Up to eight NFA paths now occupy separate eight-bit lanes in its existing
256-word table. OR-ing every lane's start bit makes a shifted bit from a
preceding lane irrelevant; masks clear padding when width is below eight.
The eighth lane's accept bit may be bit 63. Signed bitwise operations preserve
that bit; no arithmetic assumption clears it.

If those paths expand to at most eight ASCII literals of the same 3–8-byte
width, search uses `memchr::memmem::find` on borrowed byte windows. The new
internal primitive accepts a byte view and a borrowed string, allocates
nothing and reports a byte offset or -1. It works on invalid UTF-8 and NULs.
Plenty's SIMD API does not provide byte-vector search, so the native primitive
uses memchr's SIMD search and portable fallbacks; extraction, batching, engine
confirmation and resource policy remain Plenty. No Rust regex matcher ships.

Extraction is bounded to 2,048 visits and at most 17 pending paths. Larger
sets, broad classes, unsupported paths and tails retain the scalar filter.
A 64-byte scalar probe stays in the entry function so dense matches avoid
both native setup and an extra Result-return helper. A separate scalar tail
avoids mutually recursive tail calls with borrowed views. Native windows
contain at most 1,024 bytes and overlap by width minus one.
Each needle searches at most that window; a hit narrows later searches to
positions no later than the best candidate. Batch work reserves `1024 * count`
before searching and charges each actual search span, including no-match
windows. Thus neither repeated suffix rescanning nor native work can escape
the limit. Work counts depend on the selected engine, as documented; an early
match can charge a whole window. Extraction and literal allocation remain
fallible, and cursor searches allocate no new storage.

### Experiments and remaining scalar code

A backward bit-parallel skip prototype used 65,839,200,426 instructions,
**5.73% more** than the rebuilt baseline, and was rejected. Packed alternatives
alone used 62,211,788,890 (about 0.09% less); most of the final improvement comes
from avoiding scalar scans on long eligible inputs. A sampling profile of the
packed-only build attributed 16.34% of cycles to `prefix_candidate`, motivating
the native experiment; sampling percentages are attribution, not a speedup.

| Standalone kernel | Plenty, checked | C, GCC 16.2.1 -O2 |
|---|---:|---:|
| `prefix-index.plenty` / `prefix.c` | 19 | 11 |
| `prefix-packed.plenty` / `prefix-packed.c` | 19 | 12 |

The original +8 attribution remains: mask bounds comparison/branch +2,
redundant byte extension +1, separate address +1, separate mask load +1,
shift/OR rather than LEA +1, loop-back jump +1, mask-length spill/reload +2,
and one fewer induction update -1. The packed C loop needs one additional
instruction for its variable start mask; its Plenty loop still uses 19.
Production counts differ with register pressure and inlining: the final
`prefix_candidate` probe loop has 22 instructions and `prefix_scalar_candidate`
tail has 21. Inspect each with `--dump-std --dump-filter`. The native algorithm
avoids most long scalar scans, while these fallback loops retain their checks.

The checked-in `clif/prefix-{a,b,c,d}.clif` isolate the remaining scalar gap:

| Variant | Change | Instructions per byte |
|---|---|---:|
| a | Original optimized CLIF | 19 |
| b | Use the byte's zero-extension to bypass negative-index normalization; keep short-table bounds failure | 18 |
| c | b, omit bounds branch under an assumed 256-entry table extent | 14 |
| d | c, express shift/OR as shift/add | 14 |

**c is diagnostic only:** the compiler does not yet prove that extent, and no
production check was removed. b removes an address instruction but replaces
two stack operations with register moves. c additionally removes the bounds
pair and those moves. d does not produce a LEA. Redundant extension and other
instruction-selection/layout costs remain backend work. These variants and
attribution are an upstream reproducer, not a patch to pinned dependencies.
General unsigned-range/table-extent proofs and backend investigation are
tracked separately in issue #760.

```sh
cargo build --locked --release --manifest-path experiments/kernels/harness/Cargo.toml \
  --target-dir target/kernel-harness
target/kernel-harness/release/plenty-kernel-harness \
  experiments/regex/clif/prefix-{a,b,c,d}.clif \
  | python3 experiments/kernels/loops.py -v
```

### Compilation cost

Application compilation (`PLENTY_CACHE=off`), including all reachable regex
code, with the baseline and candidate compilers:

| Compiler | Run 1 | Run 2 | Run 3 |
|---|---:|---:|---:|
| Baseline | 1,039,733,420 | 1,040,047,465 | 1,037,570,247 |
| Candidate | 1,105,237,651 | 1,098,649,013 | 1,097,911,485 |

Median application compilation increases **5.67%**. Spreads are 2,477,218 and
7,326,166 instructions respectively. Runtime pattern compilation is included
in the whole-program figures, not this table. The additional compiled metadata
and extraction code explain the tradeoff.


### Adversarial complement

`prefilter-workload.plenty` scans 200,000 bytes using a reused cursor and prints
match counts 1, 0 and 25,000 for sparse, near and dense modes. Outputs match the
baseline. These are single-run instruction probes, separate from regex-redux.

| Input | Baseline | Final | Change |
|---|---:|---:|---:|
| Sparse: `z` bytes then one prefix | 9,757,512 | 7,345,555 | -24.72% |
| Near: `a` bytes, neither `aaaaaaab` nor `baaaaaaa` | 19,545,190 | 7,328,783 | -62.50% |
| Dense: `a` bytes, `aaaaaaab|aaaaaaaa` | 77,994,609 | 80,953,268 | +3.79% |

The initial native-only candidate used 116,763,268 instructions on the dense
probe (+49.71%). A helper-based scalar probe reduced this to 90,922,131;
keeping the probe in the entry function produced the final result above.
The remaining dense-case cost is a limitation, not a claimed improvement.
Bounds and exact budget regressions include an overlapping batch/tail at
`i64.MAX`, eight packed lanes including bit 63, and raw non-UTF-8 input.


The final candidate's general `compile_bench 1000 10 --workload aot` runs,
with normal warm-up and `PLENTY_CACHE=off`, retire 133,856,935,718;
133,855,934,490; and 133,883,071,297 instructions (spread 27,136,807).
These are candidate-only qualification runs, not a comparative improvement
claim. The application compilation table supplies the before/after comparison.
The stripped regex-redux executable grows from 482,632 to 519,944 bytes.
