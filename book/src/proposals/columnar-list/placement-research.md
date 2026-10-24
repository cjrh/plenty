# Why identical scan code can run at different speeds

Online follow-up to issue #437, researched on 2026-10-24. The research found
a closely matching upstream incident and a plausible explanation of the measured
cycle groups, but **no verified small fix or dependency upgrade**. My recommendation
is to close this investigation as understood enough to defer, retaining its
reproducer and evidence. That is not a claim that performance was fixed or that
the exact CPU mechanism has been proved.

This report adds no benchmark runs or compiler changes. It interprets the
[existing measurements](alignment-policy.md) and inspects the retained scan's
disassembly.

## What AMD documents

AMD's Zen4 guide describes next-address generation in aligned 64-byte blocks;
a branch target near the end leaves a short block and reduces fetch bandwidth.
For very hot loops it discusses aligning the loop's start or end, and keeping
branch placement within predictor limits. These concern the loop itself, not
merely the enclosing function's address. See sections 2.8.1.1 and 2.8.3 of
[AMD publication 57647, revision 1.01, April 2023 (mirrored PDF)](https://www.numberworld.org/blogs/2024_8_7_zen5_avx512_teardown/57647_zen4_sog.pdf).

The same guide distinguishes that next-address path from traditional instruction
fetch/decode: section 2.9 specifies 32-byte fetches aligned to 16-byte boundaries
and contained within a 64-byte block. Section 2.9.1 describes a decoded-op cache
that bypasses ordinary decoding and has its own entry and transition limits.
Consequently, warm instruction caches and equal retired-instruction counts do
not establish equal instruction-delivery throughput. The document is AMD's
primary material, hosted by a mirror; AMD's current document endpoint returned
an authorization error during this research.

## A simple model fits the retained scan

For ordinary successful iterations, the 294-byte function executes two separated
instruction regions:

- The condition and taken branch occupy offsets `0xc6..0xce` inclusive.
- The body, untaken overflow branch and jump back occupy `0xfd..0x117` inclusive.

The function's exit epilogue lies between them. Consider a deliberately simple
model: count the 64-byte blocks touched by **each region separately**, then add
the counts. Do not count only unique cache lines across the whole loop: a taken
branch starts the next region. For function address modulo 64 equal to `d`, a
region `[a, b]` touches `floor((d+b)/64) - floor((d+a)/64) + 1` blocks.

| Function address modulo 64 | Condition fragments | Body fragments | Sum | Measured cycles/row group |
| --- | ---: | ---: | ---: | ---: |
| 9, 16, 25, 32 | 1 | 1 | 2 | about 2 |
| 0, 41, 48 | 1 | 2 | 3 | about 3 |
| 57 | 2 | 2 | 4 | about 4 |

This arithmetic matches the rounded median cycle count of all 64 variants in
the retained `one` and `repeat` sweeps in
`experiments/columnar-list/plenty/alignment-policy-measurements.json`. It also
explains why 64-byte function alignment selected the three-cycle group: it fixes
`d = 0`, leaving the body to straddle a boundary. This is new analysis of existing
data, not a new experiment or an AMD-specified cycle formula.

**Inference, not attribution:** the correspondence makes instruction delivery a
strong candidate. It does not separate fetch/prediction sequencing, op-cache
packing, decode/fusion behavior, or other placement effects. The experiment moved
other code too, and collected no counters isolating these mechanisms. Do not
generalize this two-region formula to arbitrary loops or label it an instruction
cache miss penalty.

## A close Cranelift precedent

[Wasmtime upstream issue 8573](https://github.com/bytecodealliance/wasmtime/issues/8573)
reported roughly twice the execution time for identical hot-function bytes after
a different function changed size. It used an Intel i5-9500T, so it is an analogy,
not a diagnosis of our AMD CPU. Maintainers specifically identified a loop split
around its exit epilogue, suggested better block layout, and discussed partial
fetch blocks. The eventual mitigation was to prefer 32-byte function alignment;
the current [x86-64 backend source](https://docs.wasmtime.dev/api/src/cranelift_codegen/isa/x64/inst/mod.rs.html)
still links that incident beside `preferred: 32` and `minimum: 1`.

This precedent is unusually close to our assembly shape. It also explains why
simply adopting that historical mitigation is insufficient here: the
[local evaluation](alignment-policy.md) already tested 32-byte alignment and
found both favorable and unfavorable placements. We have not identified a new
fix hidden behind that source comment.

As checked on the research date, the published
[Cranelift shared settings](https://docs.wasmtime.dev/api/cranelift_codegen/settings/struct.Flags.html)
identified themselves as `0.138.0-dev`. They expose minimum *function* alignment
and dummy basic-block padding for debugging, but no loop-alignment setting.
The latter padding remains unsuitable as a production fix. This API check and
the upstream searches found no verified upgrade remedy; they do not prove that
newer code generation could never improve this case. No newer backend was built
or timed for this research.

## Decision

There is useful explanatory evidence, so the outcome is not “nothing was found.”
There is still no bounded, validated compiler change to make. Closing issue #437
as investigated/deferred is reasonable under the requested effort limit. Preserve
the address-sensitive benchmark as evidence; do not describe closure as a
performance fix, dismiss the observation as noise, or adopt global padding.

If this becomes important again, the concrete question is whether keeping the
hot loop together, or applying loop-aware alignment in the backend, improves
multiple workloads and architectures. The upstream incident supports that
direction, but does not supply a drop-in patch. It does not justify switching
backends solely for this microbenchmark.

For future causal diagnosis, AMD's
[uProf pipeline-utilization documentation](https://docs.amd.com/r/en-US/57368-uProf-user-guide/4.7.1.-Pipeline-Utilization)
separates front-end bandwidth and latency bottlenecks, branch misprediction,
and SMT contention. Those measurements would distinguish hypotheses better
than repeating cycles and instruction counts alone. They were not collected
here, and no additional local measurement campaign is proposed by this report.
