# Transitive caller-context analysis (issue #948)

`view-K-N.plenty` isolates `std.bytes.to_text(&sequence[start:stop])`,
including view creation, fallible conversion and release. The companion
[`key-construction` kernels](../../key-construction/README.md) exercise the
range-checking `Bytes.to_text_range` wrapper. Main baseline: `caf7c247`;
release compiler, optimized output, cache disabled, Ryzen 7 7840HS, CPU 4.

## Conversion kernel

Instructions per pass are the difference between N=2,000,000 and N=1,000,000.
CPU time uses the same difference of `task-clock`, seven repetitions per
program. These short measurements share the machine with compiler work;
instruction counts are more stable than time. Raw counter reports are in
[`measurements.json`](measurements.json).

| Kernel | Baseline instructions | Transitive proof | Baseline ns | Proof ns |
|---|---:|---:|---:|---:|
| View, 3 bytes | 130.03 | 97.00 | 7.99 | 5.80 |
| View, 12 bytes | 375.04 | 342.00 | 16.65 | 14.51 |
| C floor, 3 bytes | 87.00 | — | 6.86 | — |
| C floor, 12 bytes | 196.00 | — | 12.13 | — |

The C floor is [`keys.c`](../../key-construction/keys.c), built with `gcc -O2`.
It checks a range, rejects non-ASCII bytes and creates a packed/heap key; it
has neither Plenty's complete UTF-8 behavior nor its public error/ownership
contract. It is a comparison floor, not an equivalent implementation. The
remaining heap-key difference includes the general UTF-8 conversion, managed
header, ownership and error representation; these are outside this change.

## Attribution and IR variants

Both the generated `ops` and optimized CLIF were dumped with `--dump-std`.
The conversion helper bodies and native conversion are identical. The change
is in the caller: direct calls to the conversion chain need no context scope.
The original `plenty_caller_enter` and `plenty_caller_leave` execute 13 and 8
instructions respectively. Their call arguments, preserved results and saved
context cause another approximately 12 instructions per pass, including the
changed register allocation and loop layout. Together they explain all 33
removed instructions. Native conversion, UTF-8 validation and release remain.

`experiments/kernels/loops.py` reports 50 versus 39 instructions in `main`'s
innermost loop; its branch-path heuristic differs from the actual reset path,
so the perf difference, rather than that static number alone, is the total.

| Generated IR variant | View k=3 | View k=12 | Decision |
|---|---:|---:|---|
| Existing local-only accessor proof | 130.03 | 375.04 | Baseline |
| Transitively prove nontrapping native operations and direct callees, including code-free owned values | 97.00 | 342.00 | Kept |
| Remove context from every direct call | — | — | Rejected: nested traps, indirect callbacks and destructor-capable cleanup require it |

[`variants.clif`](variants.clif) additionally isolates the ABI effect with an
opaque one-word conversion result. The existing CLIF harness reports 17 loop
instructions with the scope and 7 without it, excluding instructions inside
the three called helpers. This model isolates register preservation and call
setup; its simpler result ABI is not the full conversion kernel above.

The proof computes the greatest fixed point of the direct-call graph.
Recursion is safe only when no operation or outgoing edge reads the context.
A native operation that can trap, unknown operation/callee, indirect call,
generator or type whose release invokes compiled code rejects that proof.
A second proof allows an inlined std body's own traps to name its user caller
in immutable site records, while direct callees must pass the first proof.
No trap is removed or moved; exact diagnostics are regression-tested.

## Whole program and compile cost

Current serial k-nucleotide already uses borrowed dictionary keys (issue #949),
so its hot loop no longer calls the conversion chain. With fasta 50,000,
two rounds of seven repetitions, reopening stdin for every run:

| Compiler | Instructions | CPU ms |
|---|---:|---:|
| Baseline | 706.17–706.28M | 71.14–72.46 |
| Transitive proof | 706.10–706.30M | 71.30–71.38 |

Outputs are identical. There is no measurable improvement in this current
benchmark. Cache-disabled optimized compilation takes 454.38M versus 458.51M
instructions (+0.91%), 221 versus 226 ms CPU in three repetitions; timing
spread is 2.6–4.3%.

## Explicit combination with issue #958

Integration branch `codex/integration-958-948` combines analysis commit
`185454fe` with #958 production commit `28a78c32` and its test-only correction
`f989012e`. Neither issue branch contains the other's changes. The 68
trap-location integration tests pass on this combination, including exact
range-trap diagnostics in optimized and unoptimized builds.

The single-call-site `key-construction/keys-K-N.plenty` kernel now inlines the
range wrapper and removes its outer context scope. Nine repetitions, same CPU,
2M–1M difference; a later measurement round under concurrent compiler/test load:

| Kernel | Baseline instructions | Combined | Baseline ns | Combined ns |
|---|---:|---:|---:|---:|
| Range, 3 bytes | 148.03 | 103.00 | 16.47 | 13.05 |
| Range, 12 bytes | 393.04 | 348.00 | 35.21 | 30.03 |

These CPU times belong to this later round, not the earlier view table.
Inlining and context elision both contribute to the 45-instruction reduction;
it is not an analysis-only result. The emitted main body has no call to
`Bytes.to_text_range` and no context scope at that conversion; the one scope
remaining in its main body belongs to initial buffer construction.

**Scope remaining after the first partial delivery (now addressed below):** the dictionary `built` kernels have three
static range-conversion sites and exceed the existing shared-inlining budget.
The larger k-nucleotide graph also prevents the terminal conversion chain from
being selected for complete inlining. Both keep the native range-wrapper
boundary and its context scope, even with #958. No size limit was relaxed.
An owned-key k-nucleotide variant (replace the borrowed lookup with
`key = sequence.to_text_range(i, i + length)?`, then `get_or_insert(key, 0)`,
and add `std.bytes.BytesError` to affected error unions) demonstrates that limit:

| Compiler | Instructions | CPU ms |
|---|---:|---:|
| Baseline | 932.41–932.59M | 183.12–183.43 |
| Analysis alone | 932.54–933.11M | 186.06 |
| Combined | 931.90–932.29M | 183.50–187.54 |

Outputs match byte for byte. These results establish no whole-program CPU
improvement. The variation in instructions includes randomized hash layout.
A future extension would need to preserve the containing frame while lowering
a retained terminal callee as an ordinary call plus an inline return; allowing
an actual tail transfer inside an inlined wrapper would return past its caller.
That ownership, ABI and diagnostic change is deliberately outside this PR.
At that stage issue #948 stayed open for the retained boundaries. Merge #958 first if reviewing
the combined single-site range benefit; the analysis itself is independently
compilable and useful for the view-conversion path.

## Reproduce

From the repository root, build the release compiler and use separate output
paths for each compiler variant:

```sh
RUSTC_WRAPPER= cargo build --locked --release --bin plenty
PLENTY_CACHE=off target/release/plenty --compile \
  experiments/kernels/caller-context/view-3-1000000.plenty -o /tmp/view-1m
PLENTY_CACHE=off target/release/plenty --compile \
  experiments/kernels/caller-context/view-3-2000000.plenty -o /tmp/view-2m
perf stat -r 7 -e instructions:u,task-clock taskset -c 4 /tmp/view-1m
perf stat -r 7 -e instructions:u,task-clock taskset -c 4 /tmp/view-2m
PLENTY_CACHE=off target/release/plenty --dump ops,clif-opt,asm --dump-std \
  experiments/kernels/caller-context/view-3-1000000.plenty -o /tmp/view-dump
python3 experiments/kernels/loops.py < /tmp/view-dump/asm.txt
# Build the existing CLIF harness using experiments/kernels/README.md.
target/kernel-harness/release/plenty-kernel-harness \
  experiments/kernels/caller-context/variants.clif > /tmp/context-variants.txt
python3 experiments/kernels/loops.py < /tmp/context-variants.txt
gcc -O2 experiments/key-construction/keys.c -o /tmp/c-keys
perf stat -r 7 -e instructions:u,task-clock taskset -c 4 /tmp/c-keys 3 1000000
```

For the whole program, generate fasta once, then use a shell redirect inside
`perf stat -r` so each repetition reopens the file. Do not pass one shared
stdin descriptor to all repetitions.

## Retained-call wrapper plan (remaining issue #948)

The follow-up uses baseline `6a466afb` (both partial fixes merged), the same
optimized release flags, CPU 4 and cache-disabled compilation. It keeps every
existing size limit. Type-only `Reborrow` and `ForwardsReferences` operations
emit no code and now count as markers. Planning first attempts full expansion;
when that does not fit, a cleanup-free wrapper can keep its proven
context-independent direct callees native. A terminal callee then uses a normal
call followed by the wrapper's inline return, including normal hidden result
storage. It never tail-transfers out of the containing frame.

| Planning variant | Wrapper operation cost | Three copies | Result |
|---|---:|---:|---|
| Original full expansion | No admissible terminal target | — | Native wrapper and caller scope |
| Retain only the conversion | Above 24 after expanding `within` | — | Native wrapper and caller scope |
| Retain context-independent predicate and conversion | 22 | 66 of 256 | Inlined wrapper, native helpers, no range caller scope |

The three static sites in `dictionary-hits/built-*` all now inline the range
wrapper. The owned-key `count_frames` graph also loses its `plenty_caller_enter`
and `plenty_caller_leave` around `Bytes.to_text_range`, while retaining
`std.bytes.text_in`. The current borrowed-key benchmark continues to bypass the
conversion. Managed wrapper inputs/locals, unknown or trapping terminal
callees, indirect terminal calls and recursive expansion remain excluded.
Unit tests cover cleanup/context/growth exclusions and recursion; integration
tests cover three sites and a retained callee returning an inline
`Result[range[i64], AllocError]` through the containing caller's result area.
All 68 exact trap-location tests pass, including invalid ranges at all three
sites, callbacks, generators and destructor paths in both optimization modes.

### Instructions, CPU and attribution

Two alternating rounds, seven repetitions per kernel, N=2M minus N=1M;
[raw counters](retained-measurements.json) include all outputs and dispersion.
Randomized dictionary layout changes probe counts between processes, so ranges
are more honest than a single exact instruction count.

| Kernel | Before instructions/pass | After | Before ns/pass | After |
|---|---:|---:|---:|---:|
| Built dictionary key, 3 bytes | 289.7–292.3 | 262.0–267.1 | 13.7–14.1 | 12.2 |
| Built dictionary key, 18 bytes | 608.7–612.6 | 581.0–583.6 | 27.0–27.9 | 28.3–28.8 |
| C dictionary-hit floor, 3 bytes | 158 | — | 16.9 | — |
| C dictionary-hit floor, 18 bytes | 208 | — | 17.7 | — |
| C key-construction floor, 3 bytes | 87 | — | 6.9 | — |
| C key-construction floor, 18 bytes | 224 | — | 13.3 | — |

The C floors are the existing `dictionary-hits/hits.c` and
`key-construction/keys.c`, built with `gcc -O2`; the former omits key
construction, the latter omits the dictionary and supports ASCII rather than
full UTF-8. Neither is an equivalent substitute. Their useful work, hashing,
UTF-8 and ownership differences are unchanged by this compiler change.

| Changed cause | Net instructions per key | Evidence |
|---|---:|---|
| Caller-context helper bodies | −21 exactly | 13 in enter, 8 in leave; pair absent in emitted loop |
| Wrapper frame/argument/result moves, retained helper calls and caller register/layout changes | About −9 together | Range wrapper disappears; predicate and conversion remain native |
| Native conversion, dictionary hashing/probing and release | Unchanged except randomized probe layout | Same runtime operations and input bytes |

The static innermost-loop heuristic reports 43→50 instructions for the counting
loop because several operations move from the wrapper into its caller. It
excludes all called bodies and does not follow every success-path branch;
the whole-process N-difference is the dynamic total. The newly retained
predicate adds a call and frame where its comparison used to inline. This
tradeoff reduces instruction count but explains why heap-key CPU time can be
slightly worse. This change establishes no uniform runtime-speed improvement.
The earlier [`variants.clif`](variants.clif) remains the ABI isolation of the
scope pair; the table above records the independently tested wrapper-plan
variants rather than attributing their effect to that simpler CLIF model.

For whole programs, five repetitions at fasta 50,000 in two alternating rounds:

| Program | Before instructions | After | Before CPU ms | After |
|---|---:|---:|---:|---:|
| Current borrowed keys | 699.89–700.06M | 700.00–700.23M | 67.43–67.74 | 68.39–71.34 |
| [Owned-key comparison](owned-knucleotide.plenty) | 950.77–951.23M | 901.29–901.86M | 82.63–85.35 | 79.12–82.50 |

At fasta 500,000, seven repetitions each in before/after/after/before order:
7.332–7.336B→6.843–6.846B instructions (about 6.7% fewer). Paired CPU means
were 514.54→509.42 ms and 585.84→557.39 ms, with relative dispersion between
1.21% and 10.26%. These CPU readings do not establish a reliable whole-program
speedup. Every measured output matches byte for byte. The owned comparison
intentionally retains per-key allocation; recommended benchmark source still
uses borrowed dictionary lookup.

After the final recursion-guard audit, complete generated asm dumps of the
built k=3 kernel and owned whole program match the measured candidate byte for
byte. The guard rejects every member of a mutual tail cycle and prevents a
completed recursive branch from enabling the retained-call fallback elsewhere.

Final cache-disabled optimized compilation of the owned comparison, two
alternating rounds of five repetitions: 432.598–432.611M→432.634–432.658M
instructions (about +0.01%), 193.51–201.39→198.23–200.94 ms CPU. Earlier
pre-audit compile counters remain in the raw report for provenance; these final
rounds supersede them. This is the measured whole-program compile cost, not a
claim about every compiler workload.
Stripped executable sizes are 212,248→212,392 bytes for built k=3 and
396,384→396,448 bytes for the owned comparison.

To reproduce, compile the existing `built-K-N.plenty` programs with each release
compiler, compile `owned-knucleotide.plenty` and the current borrowed benchmark,
and use the `perf` commands above with `taskset -c 4`. Generate fasta input with
`benchmarks/game/fasta/reference/fasta.rs`; reopen the input inside each perf
repetition. Dump each owned program with `--dump ops,clif-opt,asm --dump-std`
and inspect `count_frames`; the original range pair is present only before.
All scope removal here is conditional on the transitive proof, and no trap or
cleanup operation is removed.
