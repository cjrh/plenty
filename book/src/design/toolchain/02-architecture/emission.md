# Specialization and native emission

Generic declarations produce cached concrete specializations. Names, types, and
operations are resolved before Cranelift receives IR; runtime metadata describes
known storage and operations, not dynamic inference.

Placeholder identities and queued concrete signatures break recursive checking
dependencies. A compilation allows 16,384 generic function/method instances in
total, with at most 256 newly required instances along a dependency chain.
Independent calls start at depth one, even when they specialize the same
template with different types. A new instance inherits its first requiring
caller's depth plus one; lifted closures and generated call wrappers preserve
that depth. Repeated calls, ordinary recursion, and alias-equivalent arguments
reuse cached specializations without spending either budget. The depth check
guards expansion, rather than counting the unrelated generics in a library.
It is a conservative implementation bound, not a proof of infinite recursion;
an unusually long finite dependency chain can also reach it. The separate total
budget bounds broad expansion and reports a resource-budget diagnostic.

The independent limits of 256 generic data instances and 256 concrete generator
consumer specializations per compilation are unchanged. Type nesting is still
bounded separately.

Native emission builds and compiles each function on any available core. The
main thread places finished functions in the object in declaration order,
preserving byte-for-byte output across core counts. There is no worker-count
flag. Tests compare one, two, and eight workers. A function compiled before,
by this or any other program, is taken from the
[compiled-function cache](cache.md) instead; the object is the same either
way.

For ordinary user functions, each worker reuses the previous compilation's
cleared Cranelift `Function` buffers as well as its builder and compilation
contexts. Function names, signatures, instructions, source locations and symbol
references are rebuilt; only storage survives. Both cache hits and misses clear
the function before reuse. Storage lasts until the worker finishes, and a large
function can leave larger buffers for later small functions. Generator bodies
and generated adapters still construct fresh IR. See the
[allocation measurements](../../../development/profiling.md#reusing-function-storage).

Workers claim one job at a time to balance functions of different sizes, but
send completed results in batches of up to 32 jobs. This reduces result-channel
wakeups on warm-cache builds. A partial batch is sent when a worker finishes or
stops after an error; the main thread still places each job in declaration order.
Batching can delay placement until the worker fills its batch and retains up to
31 additional completed jobs per worker before sending them.

With the cache enabled, a result keeps a shared owner of the cache's immutable
machine-code bytes until placement, avoiding a separate allocation and copy for
each job. The object builder still copies those bytes into its own sections.
Relocations remain specific to each function and are resolved independently.
This adds no cache retention: decoded stencils already live until the compilation
ends. With caching disabled, each result owns its machine-code buffer.

The `plenty` and `plentyc` executables allocate with mimalloc (version 2,
without transparent huge pages) instead of glibc malloc (issue #315). Workers
free operations the main thread allocated and grow IR buffers at a high rate;
glibc serializes such frees and growth on per-arena locks, so threads slept on
each other. Emitting 1,000 units from a warm cache on 16 hardware threads made
about 86,000 voluntary context switches with glibc and 480 with mimalloc. The
compiler library does not choose an allocator, so tests and other programs
that call it in-process keep their own.

A command-line check (`--check`, `--check-module`, and `--package-graph
--check`) leaks the checked program instead of freeing it, since the process
exits next (issue #321). On the 1,000-unit benchmark this saves 2.0% of
instructions and about 4% of cycles; peak memory is unchanged. Only the
single-file compiler interface enables it, through `plenty::skip_teardown`,
and not when it runs the program it compiled: `plenty lsp`, project commands,
tests and library callers free every program. Other allocation strategies
were measured on 2026-10-20 and not adopted:

- Leaking the operation tables and object at the end of emission saved 0.02%
  of `--emit-object` instructions: workers already free each function's
  operations once its code is built.
- A never-freeing bump allocator, the upper bound for arenas, saved 10.5% of
  checking instructions and 2.7% of emission instructions, but took 30–45%
  more task time from page faults, and raised peak memory from 88 MB to
  354 MB for checking and from 97 MB to 1.9 GB for emission.
- The allocator itself takes 8% of checking cycles and 6% of emission cycles.
  The largest sources in checking are lexer tokens and type clones in the
  frontend, tracked by issue #302; most emission allocation is inside
  Cranelift, whose per-function register allocator context Plenty cannot
  reuse without patching it (issue #153).

A body can need symbols that are declared on first use: type descriptors, its
trap-site table, foreign imports. Their order decides the object's bytes, so a
worker building IR does not declare them. It names each in a placeholder
namespace, and the main thread declares them in declaration order as it places
the function, then points the function's relocations at them. If any function
fails, emission reports the error of the first failing function in that order.

## Match dispatch

Integer literal and lowered enum-tag dispatch uses a jump table when at least
256 distinct keys form one contiguous range (issue #395). Other sets retain
source-order comparison chains. This conservative threshold follows the
[dense-match measurements](../../../development/profiling.md#dense-match-dispatch);
small tables regressed for some input distributions.

The lowerer sorts keys with their original arm blocks, checks contiguity and
rejects duplicates or an early wildcard before selecting a table. It subtracts
the minimum key with wrapping arithmetic at the scrutinee's width, then checks
the unsigned index against the last valid entry **before** reducing to the
32-bit index Cranelift's `br_table` takes. This preserves negative ranges,
ranges crossing zero, unsigned maxima, and full 256-value byte ranges without
letting distant 64-bit values alias an entry. Storage is proportional to the
number of cases, never an unbounded sparse key span.

The default edge remains the wildcard or the defensive invalid-tag trap. Only
dispatch changes: arm bodies, payload loads, ownership cleanup, source-order
payload subpatterns, joins and tail exits use their existing lowering paths.
No source syntax, allocation behavior or target CPU requirement changes.

## Symbol references

Code is compiled position-independent (Cranelift's `is_pic`), so the object
links into a PIE or a shared library. How code refers to a symbol depends on
where the symbol is defined (issue #372):

| Target | Reference | Relocation |
|---|---|---|
| Program function, runtime helper | `call rel32` | `R_X86_64_PLT32` |
| Data the object defines locally (literals, trap-site tables, type metadata, coverage counters) | `lea sym(%rip)` | `R_X86_64_PC32` |
| [Trusted C declaration](../../facilities/04-interop/trusted-declarations.md), `memmove`, `memset` | load from the GOT, indirect call | `R_X86_64_GOTPCREL` |
| Address of a program function | load from the GOT | `R_X86_64_GOTPCREL` |

Runtime helpers are marked colocated: the runtime archive is linked into
every executable and shared library made from the object, and a shared
library's version script makes the runtime's symbols local, so the linker
resolves the call directly, without a PLT entry. A C library may be shared,
so its functions stay behind the GOT.

Under `is_pic`, Cranelift loads every symbol address from the GOT, even a
colocated one, and the object's plain `R_X86_64_GOTPCREL` relocations are not
relaxed by the linker. After compiling a function, the compiler therefore
rewrites each GOT load of colocated data into a `lea` with a PC-relative
relocation (`src/codegen/relax.rs`), the rewrite a linker applies to
`R_X86_64_REX_GOTPCRELX`. Only data no other module can preempt is
colocated. The rewrite depends only on the code and the function's IR, so it
applies equally to [cached](cache.md) code, and the cache key, which covers
the IR, still identifies the result. Function addresses, taken for function
values, closures, generators, and runtime callbacks, are not rewritten.

On `examples/http_server_bench.plenty` with the standard library (779
functions, release), this removed 42,389 of 42,760 GOT loads and 35,220 of
35,590 indirect calls, and reduced machine code from 1,888,952 to 1,730,245
bytes (8.4%).

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

## Call-site substitution

Before any body is lowered, `src/codegen/small_calls.rs` replaces two kinds of
call with the callee's operations (issue #363):

- **Constants.** A read of a module `const` becomes a literal of the
  constant's declared type, so width, sign and float bits are those of the
  declaration. This happens in every build, and the constant has no function
  in the object.
- **Leaf predicates**, in release builds only. A function whose parameters
  and result are numbers or `bool`, with no locals, and whose body is at most
  12 operations drawn from parameter reads, literals, constants, comparisons,
  `not`, `and`, `or`, and matches on `bool` or integer literals. Nothing in
  such a body can trap, call, allocate, or touch memory. The caller stores
  the arguments in new local slots and runs the body; calls to one predicate
  in a body share its slots. The definition is still emitted, since a
  function value may name it.

Arithmetic, which can overflow, any call, and recursion keep the call.
Generator bodies keep predicate calls, since their frame layout is fixed by
then, as does `plenty_main`.

The substituted operations report no source position, so a later trap or
call in the caller reports the same position and call chain as before. They
become part of the caller's IR, so the [compiled-function cache](cache.md)
key changes when a constant's value or a predicate's body does.

## Last-use moves

After substitution, `src/codegen/last_use.rs` turns the last read of a local
into a move (issue #388). A read of a local counts a new owner, and the
local's own owner is released later, when it is reassigned or the function
returns. When no later operation can read the local, the read takes its value
and leaves an empty slot. A `match` on an owned parameter and a payload binding
read once are the common cases.

An empty slot holds zero, and the release of a value known to be zero emits
nothing (issue #391). Every write of an empty slot in a function uses the same
one-word and two-word zero constants from its entry block, so where all paths
into a join leave a slot empty, the slot's value there is that constant and its
later release emits nothing. A slot moved from on only some paths keeps its
release; for a two-word value it tests for zero before calling the runtime.

A backward liveness pass over each function finds these reads. Loops are
solved to a fixed point; `break`, `continue`, early returns and the error path
of `?` are followed. Only managed copyable types qualify: they have no
destructor, so ending the last owner earlier changes no output. Locals that
are ever borrowed and generator bodies are skipped. A one-word read that feeds
a collection query keeps the borrowed read from issue #379, which costs less
than a move.

When a `match` reads the tag of a local it has just copied, the copy's counting
is removed, so testing an owned scrutinee costs no runtime call. The pass runs
after the ownership checker, so diagnostics are unchanged.

## Constants in loops

In release builds Cranelift rematerializes an integer or float constant in each
block that uses it rather than keeping it in a register across a loop: this
saves a register for the loop's lifetime at the cost of one instruction per use.
On x86-64 a constant that is the right operand of a float operation is read from
the constant pool. A left float operand, or a 64-bit integer constant in a
`wrapping_add`, is built with `movabs` (plus `movq` for a float) each time it is
used (issue #370).

Lowering therefore moves a non-NaN constant to the right of float `+` and `*`,
which gives the same bits: only the choice between two NaN operands depends on
order. `2.0 * x` and `x * 2.0` become `x + x`, which rounds, overflows, and
propagates signed zero, infinities and NaN identically. No other float
operation is rewritten; there is no reassociation, fusion, or fast-math mode.

A non-zero constant on the left of `-` or `/`, which cannot move, is emitted as
lane 0 of a `vconst` vector constant (issue #380). Cranelift does not
rematerialize a `vconst`, and x86-64 loads it from the constant pool with one
`movupd`, so `1.5 - x` reads its constant without a general-purpose register.
The operation and its operand bits are unchanged. Zero keeps its `f64const`,
which lowers to a register-clearing `xorpd`, and a constant whose other operand
is also constant is left for Cranelift to fold. In a throughput-bound loop with
four `c - x` terms (`taskset`, `perf stat`, 10⁹ iterations, Ryzen 7 7840HS) the
change lowers cycles from 8.8 to 5.4 billion, matching the same loop with the
constants passed as arguments; in a loop bounded by a dependency chain the
`movabs` was off the chain and the cycles do not change.

`x.fabs()` and `x.copysign(y)` are bit masks rather than runtime calls: `band`
with the magnitude mask, and `bor` of that with `band(y, sign mask)`. Every
mask and constant is a right operand, which x86-64 reads from the constant
pool; Cranelift's own `fabs` and `fcopysign` build their masks with `movabs`.
Like Rust's `abs` and `copysign`, they keep a NaN payload. A constant receiver
or sign argument is folded into the masks while lowering.

Float negation `-x` is `bxor` with the sign mask rather than Cranelift's
`fneg`, whose x86-64 lowering builds the mask with `movabs` + `movq` at each
use (issue #385). Both flip only the sign bit, NaN payload included. Lowering
folds a constant operand and `-(-x)`; Cranelift's other `fneg` rewrites, such
as `(-x) * (-y)` to `x * y`, no longer apply. In a throughput-bound loop with
four `s + -(x * a)` terms the change lowers cycles from 7.7 to 4.4 billion and
instructions per iteration from 30 to 22.

Integer constants are left as Cranelift places them. In the
`examples/cpu_codegen_audit` `mix` kernel the per-iteration `movabs` is off the
loop's dependency chain: passing the constant as an argument instead removes
the instruction but not a cycle (7.05 billion cycles either way for 10⁹
iterations). Removing the rematerialization would need a Cranelift change.

## Float rounding

`x.floor()`, `x.ceil()`, `x.trunc()` and `x.round()` are inline SSE2
sequences, equal bit for bit to Rust's `floor`, `ceil`, `trunc` and
`round_ties_even` (issue #386). Generated code targets the
[SSE2 baseline](../01-building/linking.md#reproducible-artifacts), where
Cranelift's `floor`, `ceil`, `trunc` and `nearest` are library calls rather
than SSE4.1 `roundsd`.

For a magnitude `a` below 2⁵² (2²³ for `f32`), `(a + 2⁵²) - 2⁵²` is `a`
rounded to a whole number, ties to even, because numbers in `[2⁵², 2⁵³)` are
spaced 1 apart. `round` uses that directly. `trunc` steps back one when the
rounded magnitude exceeds `a`; `floor` and `ceil` apply the sign of `x` and step
one down or up when the rounded value lies past `x`. The step is a vector
compare (`cmppd`) whose lane mask selects the constant 1, so no branch depends
on the fraction. ORing in the sign bit of `x` gives `-0.0` for a negative `x`
whose result is zero. A larger magnitude or an infinity is already whole and is
returned unchanged, behind the one branch; a NaN passes through the arithmetic,
which quiets it and keeps its sign and payload. A constant receiver is folded
while lowering.

The calls went through `plenty_f64_math` to Rust's `libm`, which rounds with
integer operations and data-dependent branches, and forced live float registers
onto the stack. With `--release`, `taskset` on one core, `perf stat`, Ryzen 7
7840HS, best of five:

| Loop (2×10⁸ iterations) | call (G cycles) | inline (G cycles) |
| --- | ---: | ---: |
| Two `floor` terms, steady step | 4.0–4.9 | 1.8–2.0 |
| One `floor` of a random value | 5.2 | 2.0 |
| One `round` of a random value | 6.8 | 1.8 |
| Dependency chain `y - y.floor()` | 7.3 | 5.0 |
| Dependency chain `y - y.round()` | 10.4 | 3.4 |

`ceil` and `trunc` match `floor`. A chain whose rounding the call's branches
predict exactly, such as `floor` of values always in `[0, 1)`, can be faster as
a call, because the prediction takes the call off the chain. Each use adds
about fifteen Cranelift instructions to compile: a file of 2,000 one-line
functions with four rounding calls each takes 55% (release) to 85% (debug)
more compiler instructions.

## Loop counter checks

Integer arithmetic keeps its overflow and division checks, and Cranelift does
not remove them, since it does not track value ranges. The lowerer drops two
kinds that a `while` loop's own condition makes impossible (issue #369,
`src/codegen/induction.rs`). Both apply to a counter kept in a CLIF variable
whose condition compares it with another local or a literal, as in
`while i < n` or `while i >= 0`:

- **Step overflow.** The first write to the counter in the body, if it is a
  top-level `i = i + k` or `i = i - k` with a literal `k`, runs at most once
  per pass, with the condition still true. `i < n` puts `i` at least one below
  the type's maximum, so `i + 1` cannot overflow; a literal bound also admits
  larger steps that stay in range, and `i > n` admits `i - 1`. Such a step is
  a plain add. Later writes, steps inside nested loops, and steps such as
  `i <= n` with a variable `n` keep their check.
- **Signed division.** A signed counter that enters the loop holding a
  nonnegative literal, and whose every write in the loop adds a nonnegative
  literal, is never negative, since each add is checked or proven. `//` and
  `%` of such a counter by a nonnegative literal or counter use unsigned
  division, with no floor correction or `MIN // -1` check. The zero-divisor
  check stays.

Counters in memory, borrowed locals and generator frames, are not analyzed.
`for` loops over ranges already step with wrapping arithmetic.

## Short circuits in conditions

`a or b` lowers to a `match` on `a` whose arms push `True` or evaluate `b`; the
arms meet in a join block that takes the `bool` as a parameter. Cranelift does
not thread branches through block parameters, so an `if`, `while`, `match`, or
enclosing `and`/`or` that consumes the result would test the join parameter
again, even on edges where an arm passed a constant.

Before Cranelift runs, `src/codegen/threading.rs` retargets each edge into a
block that only passes its parameters on or branches on them, when no other
block uses its values and it has no instruction that traps, touches memory, or
calls. An edge whose arguments decide the block's `brif` goes straight to the
chosen target; a `jump` whose argument is the unknown condition, or its `not`, becomes that
`brif`, with the targets swapped for `not` (issue #377). Only branches move, so
operands still run in order, only when needed, with their traps and `?`
propagation. A short-circuit result stored in a variable and read after the
condition keeps its join block.

## Measuring compilation

`examples/compile_bench.rs` generates units containing a protocol, class, enum,
and three functions. It reports median checking time, optionally full AOT/linking,
and a fourfold-input scaling ratio. `tests/test_compile_scaling.rs` guards against
superlinear checking of classes, functions, protocols, enum variants, and
`match` cases.

For time and allocations per compiler phase in one run, build with the
`profile` feature; see [Profiling the compiler](../../../development/profiling.md).

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
of cores, for example `taskset -c 0,2,4,6,8,10,12,14`. The 2026-10-20
measurement below did not reproduce the slowdown.

The 2026-10-15 release measurement used the same eight cores while other
builds loaded the machine, with paired runs of the compiler before and after
building IR on worker threads. Object emission of 5,000 units took 6.0 to 6.5 s
before and 5.7 to 5.9 s after (three runs each); 20,000 units took 25.8 to
26.7 s and 23.2 to 24.4 s (two each). CPU time and peak memory were unchanged.
The per-function trap-site tables added 0.4 MB of read-only data to the
5,000-unit object (66.6 MB) and 0.2% to a linked 1,000-unit program's code
and data. The remaining serial work is the frontend, about 1.9 s of the
5,000-unit emission.

The 2026-10-16 release measurement used the same eight cores, again beside
other builds, with paired runs before and after resolving names, validating
frames, preparing tail calls, and finding thread and executor adapters on all
cores. For 5,000 units these phases took 48, 19, 35, and 20 ms instead of 99,
85, 47, and 35 ms. `--check` took 1.77 to 2.02 s before and 1.62 to 1.84 s
after, and object emission 6.1 to 7.9 s before and 6.0 to 6.3 s after (five
runs each).
Peak memory was unchanged. On 1,000 units the difference was within noise.

The 2026-10-20 release measurement compared glibc malloc with mimalloc in
paired, alternating runs of `--emit-object` (no linking), with a load average
of 4 to 7 from other builds. Medians of three runs for 5,000 units, wall time
with user and system CPU time in seconds:

| Cache | Cores | glibc | mimalloc |
|---|---|---|---|
| Off | 8 physical | 2.26 (7.7 + 0.41) | 1.98 (7.2 + 0.37) |
| Off | 16 hardware threads | 2.07 (10.7 + 0.61) | 1.74 (9.9 + 0.45) |
| Warm | 8 physical | 2.11 (3.7 + 1.55) | 1.42 (2.4 + 0.49) |
| Warm | 16 hardware threads | 2.13 (4.8 + 3.16) | 1.39 (2.9 + 0.57) |

With mimalloc, voluntary context switches fell from 556,000 to 1,200 for the
warm 16-thread build, and peak memory was 375 MB against 388 to 414 MB. For
1,000 units the warm build took 0.26 s instead of 0.37 s on 16 threads, and
peak memory rose from 92 to 110 MB. Sixteen threads were as fast as eight or
faster, so the worker count stays one per available processor. In this run the
clock stayed near 4 GHz on 16 threads; sixteen threads executed the same
instructions as eight in 31% more cycles, from sharing cores.
Parsing, type resolution, class expansion, generic preparation, signatures,
and lowering remain on one thread, about 0.75 s of the 5,000-unit frontend,
as do declaring functions (0.1 s) and writing the object.

The 2026-10-17 release measurement used eight logical processors on four
cores (`taskset -c 0-7`), beside other builds, before and after resolving each
declaration once: class templates are copied without method bodies, concrete
types are defined without copying their declarations, and signatures are
registered from the types generic preparation resolved. Checking 5,000 units
spent, in milliseconds (medians of nine runs each):

| Phase | Threads | Before | After |
|---|---|---|---|
| Parsing | 1 | 279 | 279 |
| Name resolution | 1 and all | 53 | 53 |
| Type resolution | 1 | 107 | 86 |
| Public API check and class expansion | 1 | 63 | 64 |
| Generic preparation | 1 | 140 | 149 |
| Signature registration | 1 | 85 | 28 |
| Lowering bodies to operations | 1 | 465 | 462 |
| Frame validation | all | 24 | 25 |
| Independent check | all | 128 | 134 |
| Freeing the program | 1 | 121 | 124 |

`--check` took 1.51 s before and 1.46 s after (eight runs each); 1,000 units
took 0.30 s and 0.28 s (ten runs each), and a program whose imports load 21
standard modules 119 ms and 112 ms (twenty runs each). Peak memory fell from
406 MB to 404 MB at 5,000 units. Object emission of 5,000 units took 5.25 s
and 5.21 s (medians of six runs each), within noise.

Running the declaration phases, from type resolution to signature
registration, on several threads could save about 0.1 s of their 0.33 s at
most. Type resolution creates nominal identities and generic instances in
program order, and registration fills the signature table; only resolving
signatures and expanding classes read the tables without changing them, and
they would need thread-safe type tables. The parallel designs measured for
parsing and lowering are in [Frontend and checking](frontend.md). Freeing the
program after the independent check costs 0.12 s, which a process about to
exit could leave to the operating system.

Report build mode, machine, input size, repetitions, and included phases with
new results. Measure checking, IR construction, native emission, runtime
extraction, and linking separately when assessing compiler changes.
