# Specialization and native emission

Generic declarations produce cached concrete specializations. Names, types, and
operations are resolved before Cranelift receives IR; runtime metadata describes
known storage and operations, not dynamic inference.

Placeholder identities and queued concrete signatures break recursive checking
dependencies. A compilation allows 256 generic function/method instances in total
and a separate 256 generic data instances. Repeated calls and alias-equivalent
arguments reuse cached specializations.

Native emission builds and compiles each function on any available core. The
main thread places finished functions in the object in declaration order,
preserving byte-for-byte output across core counts. There is no worker-count
flag. Tests compare one, two, and eight workers.

A body can need symbols that are declared on first use: type descriptors, its
trap-site table, foreign imports. Their order decides the object's bytes, so a
worker building IR does not declare them. It names each in a placeholder
namespace, and the main thread declares them in declaration order as it places
the function, then points the function's relocations at them. If any function
fails, emission reports the error of the first failing function in that order.

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

The 2026-10-12 release measurement used the same machine restricted to its
eight physical cores, and 5,000 units / 195,003 lines. Checking function bodies
on all cores reduced the independent check from 0.48 s to 0.09 s and `--check`
from 1.81 s to 1.44 s (five runs each). Object emission then spends, in seconds:

| Phase | Threads | Time |
|---|---|---|
| Parsing and name resolution | 1 | 0.31 |
| Type resolution, class expansion, generic preparation, signatures | 1 | 0.35 |
| Lowering bodies to operations | 1 | 0.41 |
| Layout, operation validation, rest of the frontend | 1 | 0.13 |
| Independent check | 8 | 0.09 |
| Tail-call preparation; declaring functions, strings, adapters | 1 | 0.19 |
| Building IR and compiling it | 8 | 2.1 |
| Serializing and writing the object | 1 | 0.22 |

On one core, building IR takes 0.95 s and compiling it 13.1 s. On eight, the
compile step holds every core: the main thread spends 0.05 s defining compiled
functions and otherwise builds IR or compiles.

With all 16 hardware threads the same emission took 9.9 s, against 4.2 s on
the eight cores. The instruction count was the same; above about ten busy
threads this machine ran at 1.1 GHz instead of 3.9 GHz. Measure on a fixed set
of cores, for example `taskset -c 0,2,4,6,8,10,12,14`.

The 2026-10-15 release measurement used the same eight cores while other
builds loaded the machine, with paired runs of the compiler before and after
building IR on worker threads. Object emission of 5,000 units took 6.0 to 6.5 s
before and 5.7 to 5.9 s after (three runs each); 20,000 units took 25.8 to
26.7 s and 23.2 to 24.4 s (two each). CPU time and peak memory were unchanged.
The per-function trap-site tables added 0.4 MB of read-only data to the
5,000-unit object (66.6 MB) and 0.2% to a linked 1,000-unit program's code
and data. The remaining serial work is the frontend, about 1.9 s of the
5,000-unit emission.

Report build mode, machine, input size, repetitions, and included phases with
new results. Measure checking, IR construction, native emission, runtime
extraction, and linking separately when assessing compiler changes.
