# Key-only dictionary probes (issue #931)

The k-nucleotide profile attributed 83.7M instructions to `Entries::get`.
Disassembly explains one avoidable part: `Collection::probe` called it to
produce a 32-byte `Entry { key, value }` on the stack, then loaded only the key
for equality. The helper still decoded the unused value. LLVM could not remove
that work across the retained call boundary.

`Entries::key` keeps the same bitset, bounds and live-row checks and reads only
the key. Inlining this small return shape eliminates the `Entries::get` call
and its full-row return slot from the probe. No hashing, table storage, growth,
allocation failure, retention/release or reference-lifetime contract changes.
This is a Rust runtime helper fix; a Cranelift IR change cannot remove that
native helper's work.

## Measurements

Release compiler, optimized Plenty output, Ryzen 7 7840HS, pinned core 0.
Before binary: primary worktree release compiler; after: this branch's release
compiler. Both retain Cranelift 0.137.0. [Counts](counts.json) are three runs.

| Program | Before median instructions | After median instructions |
|---|---:|---:|
| [One million hit-only updates](hits.plenty) | 523,621,360 | 494,621,599 |
| k-nucleotide, fasta 50,000 | 1,453,769,510 | 1,410,768,003 |

The isolated case saves 29 instructions per hit (5.54% overall); k-nucleotide
saves about 2.96%. Both outputs match byte-for-byte. Random hash seeds affect
the larger input: before spans 1,453,526,992–1,453,964,622, after
1,410,592,281–1,410,845,793. The previous reference investigation measured native
Rust near 875M instructions; this small fix does not close that gap.

Callgrind on the hit-only kernel gives a complete changed-cost ledger (self
instructions per update, excluding one-time setup):

| Region | Before | After |
|---|---:|---:|
| `Collection::probe` | 58 | 86 |
| `Entries::get` | 57 | 0 |
| Combined | 115 | 86 |

Hash (157), equality (58), native dispatch (80), locate (51), main loop (38)
and libc comparison (19) stay unchanged. Thus all 29 removed instructions are
accounted for at the helper boundary: 57 disappear and 28 key-reading
instructions move into the probe. Callgrind totals are 530,306,964 versus
501,306,936; they differ from native perf because of startup/libc execution.

The changed machine-code region consists of call setup/return, full-row
materialization and unused value decoding. Hash/equality calls and scalar
bucket probing remain. The probe's code grows because key-reading branches
are now inline; retain this tradeoff rather than claiming smaller code.
Other dictionary/key-construction wrapper costs remain in issue #931.

[Compile instruction counts](compile-counts.json), cache disabled on
k-nucleotide, span 451.3–453.8M before and 442.7–446.6M after. These include
linking and are observations, not a frontend speedup claim.

An eleven-pair alternating [wall-time check](timings.json), with concurrent
builds, was inconclusive: medians 0.200s before and 0.210s after, with ranges
0.130–0.223s and 0.130–0.234s. Instruction reduction is established; a runtime
time improvement is not established by this noisy session.

## Reproduce

Build both compilers with `RUSTC_WRAPPER= cargo build --offline --locked
--release --bin plenty`, then compile `hits.plenty` and the unchanged
`benchmarks/game/k-nucleotide/plenty/knucleotide.plenty` with each compiler,
using `PLENTY_CACHE=off`, `--compile --no-strip`. Generate input with the
repository's Rust fasta reference and argument `50000`. Run:

```sh
python3 experiments/dictionary-probe/measure.py BEFORE AFTER INPUT target/probe.json
objdump -Cd --no-show-raw-insn BEFORE > target/before.asm
objdump -Cd --no-show-raw-insn AFTER > target/after.asm
```

Inspect `Collection::probe`: before it calls `Entries::get`; afterwards it
reads the key inline and calls equality. `measure.py` alternates counts and
checks outputs; its timing includes perf startup and is labelled accordingly.
The stored wall-time observations timed direct execution separately.

The differential runtime test covers packed scalar and ordinary ordered rows,
inline range/class keys, and removed rows. Existing dictionary allocation,
reference and hash-once tests remain applicable. This is a bounded delivery
slice; issue #931 remains open for the other measured helper costs.
