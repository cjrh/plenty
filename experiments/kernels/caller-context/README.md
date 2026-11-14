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

**Remaining scope of issue #948:** the dictionary `built` kernels have three
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
Keep issue #948 open for the retained boundaries. Merge #958 first if reviewing
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
