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

Cranelift's verifier checks compiler-generated IR, not source programs. It runs
in debug compiler builds, including tests; release builds omit it. The compiler
uses Cranelift's ordinary register allocator. A measured single-pass alternative
reduced emission CPU time but increased code size and slowed native workloads.

## Measuring compilation

`examples/compile_bench.rs` generates units containing a protocol, class, enum,
and three functions. It reports median checking time, optionally full AOT/linking,
and a fourfold-input scaling ratio. `tests/test_compile_scaling.rs` guards against
superlinear checking of classes, functions, and protocols.

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
