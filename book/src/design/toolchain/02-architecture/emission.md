# Specialization and native emission

Generic declarations produce cached concrete specializations. Names, types, and
operations are resolved before Cranelift receives IR; runtime metadata describes
known storage and operations, not dynamic inference.

Placeholder identities and queued concrete signatures break recursive checking
dependencies. A compilation allows 256 generic function/method instances in total
and a separate 256 generic data instances. Repeated calls and alias-equivalent
arguments reuse cached specializations.

Native emission builds function IR on one thread in declaration order, then
compiles batches of fixed IR size on all available cores. Definitions enter the
object in build order, preserving byte-for-byte output across core counts. There
is no worker-count flag. Tests compare one, two, and eight workers.

Building IR declares symbols in the object as it meets them: type descriptors,
trap-site records, foreign imports. Their order decides the object's bytes, so
building stays on one thread. It is about 7% of emission's CPU time, and the
building thread compiles too whenever a batch is ready before the previous one
is done, so building limits emission only above roughly fourteen cores
(measurement below).

The worker count is the number of processors the process may run on. Restricting
its CPU affinity, for example with `taskset`, restricts the workers.

A function's operations are released once its IR is built, and the object is
written to its file one section at a time. Emission therefore holds neither
every function's operations nor a second copy of the object (issue #54).

Cranelift's verifier checks compiler-generated IR, not source programs. It runs
in debug compiler builds, including tests; release builds omit it.

The compiler uses Cranelift's backtracking register allocator. Its time grows
with the square of a function's branch arguments, the values its branches pass
to block parameters: two for each variable assigned in a conditional arm, for
example. A function with more than 2,048 branch arguments is compiled with
Cranelift's single-pass allocator instead, whose time is linear (issue #56).
The largest function in the test suite has 156. Generated code reaches the
limit: about a thousand conditional assignments, two thousand `match` arms, or
a thousand-element collection literal in one function. In measurement such a
function's machine code was 60% to 135% larger and took 1.1 to 3.6 times the
CPU cycles to run. Applied to whole programs, the single-pass allocator reduced
emission CPU time but increased code size and slowed native workloads, so no
other function uses it. The choice depends on the function alone, so it is the
same for every core count. `tests/test_large_functions.rs` runs functions over
the limit.

## Measuring compilation

`examples/compile_bench.rs` generates units containing a protocol, class, enum,
and three functions. It reports median checking time, optionally full AOT/linking,
and a fourfold-input scaling ratio. `tests/test_compile_scaling.rs` guards against
superlinear checking of classes, functions, protocols, enum variants, and
`match` cases.

For the 2026-10-10 release measurement on an AMD Ryzen 7 7840HS, x86_64 Linux,
16 hardware threads: 4,000 units / 156,003 lines checked in 1.61 s and 16,000
units / 624,003 lines in 6.69 s. Parallel emission and omitting release-build
verification reduced full AOT of 1,000
units / 39,003 lines from 3.25 s to 0.87 s, and object emission of 5,000
units / 195,003 lines from 16.9 s to 4.35 s (2.6 s checking). With one core the
latter took 13.2 s, isolating verifier removal. These are workload
measurements, not general latency guarantees.

Report build mode, machine, input size, repetitions, and included phases with
new results. Measure checking, IR construction, native emission, runtime
extraction, and linking separately when assessing compiler changes.
