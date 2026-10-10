# Compiler architecture

```text
source → lexer → AST → alias resolution → signatures → local type checking
                                                       ↓
                                              typed operation IR
                                                       ↓
                                             Cranelift → object → cc → executable
                                                                ↑
                                                 embedded Rust runtime archive
```

`frontend.rs` owns modern syntax, name resolution, local inference, mutability,
and lowering. It emits operations directly, never translated legacy source.
The parser retains type references with source positions. Before collecting
signatures, the frontend resolves aliases with an iterative dependency walk,
including references inside collection arguments, and caches each concrete
result. Alias-only cycles are rejected without Rust recursion on long alias chains.
Nominal definitions reserve identities before resolving fields; weak stored edges
and table-owning handles support [recursive types](30-recursive-data.md) without
compiler memory cycles. Type facts and native metadata use finite graph walks.
All signatures and annotations normalize to existing `Ty`
values before the backend runs. The frontend returns operations only; aliases
produce no runtime operations or persistent session state.
`op.rs` remains a backend-neutral operation IR and an independent type checker.
Its compile-time operand stack is an implementation detail, not a language
feature. `CompiledFn` carries a signature, documentation, body, and local-slot
types. `LoadLocal`/`StoreLocal` address typed slots; conditional expressions
use exhaustive Boolean branches. Unit is represented by zero stack values.

Early returns add an explicit `Return` terminator. The frontend distinguishes
continuing blocks (with a result type) from blocks that exit the current path.
The independent IR checker validates `Return` and `TailCall` against the
enclosing signature, rejects operations after a guaranteed exit, and joins
only continuing arms. Cranelift emits `return`; terminated arms
do not jump to the branch join. Explicit return expressions are lowered so
each conditional path returns or tail-calls, preserving tail-call optimization.

This uses the existing structured IR, which already models terminated arms
for tail calls. Structured loops now extend it with a condition and body, lowered
to a Cranelift header, body, and exit. The checker requires a Boolean condition
and a stack-preserving continuing body. `Break` and `Continue` are terminators
checked against the innermost loop's entry stack; function returns are checked
against the function signature. The frontend emits the `for` increment before
each continue. Native lowering tracks loop headers and exits and seals blocks
after all incoming edges are known, giving mutable locals correct SSA joins.
Native lowering still consumes structured operations. Borrow analysis separately
flattens typed access facts to an explicit source-level control-flow graph.

Cranelift declares one SSA variable per slot; stores define
variables and joins use Cranelift's SSA construction. All names and types are
resolved before native emission. `PrintLine` formats modern values naturally,
and `FloorDiv` adds Python-compatible floor semantics without changing legacy
backend regressions. Strings use explicit byte/scalar lengths, including embedded
NUL; raw modern output writes exactly the stored byte length.

The public `compile_source_to_executable` uses modern syntax.
`check_source` validates an isolated binary source string; both string APIs
reject imports instead of implicitly searching the filesystem.
`compile_file_to_executable(path, output, root)`, `check_file(path, root)`, and
`check_module_file(path, root)` resolve imports from an explicit optional root.
Checking performs no execution or native emission.
`examples/compile_bench.rs` generates a repeatable workload of units, each a
protocol, a class, an enum, and three functions, and reports median checking
time, optionally including full AOT compilation and linking. It also checks four
times as many units and prints the ratio: about 4 is linear, about 16 quadratic.
It reports build mode, target, source size, unit count, and repetitions; record
machine details alongside any published result.

Initial diagnostic baseline (2026-10-05): AMD Ryzen 7 7840HS, x86_64 Linux,
debug Rust build, 100 generated functions / 6,194 source bytes, one warm-up and
five measured repetitions. Median parse/resolve/check was 1.112 ms; full AOT
including checking, native emission, C runtime compilation, and linking was
47.275 ms. This small synthetic workload is a starting measurement, not a
release-performance guarantee or a bound for larger programs.
After the ownership/reference migration, the same debug workload and repetitions
measured 1.487 ms for parse/resolve/check and 53.140 ms for full AOT. These are
single-session diagnostic measurements, not a controlled performance comparison.

Checking is linear in the number of declarations (issue #53); earlier figures
came from a 100-function workload too small to show that it was quadratic. On
the same machine with a release build (2026-10-10), 4,000 units / 156,003 lines
checked in 1.61 s and 16,000 units / 624,003 lines in 6.69 s; full AOT of 1,000
units / 39,003 lines took 3.24 s. `tests/test_compile_scaling.rs` fails if
checking classes, functions, or protocols becomes superlinear.

Native emission builds each function's Cranelift IR on one thread, in
declaration order, and compiles the IR to machine code on every available core
(issue #55). Functions are compiled in batches of a fixed IR size and defined in
the object in the order they were built, so the object is byte-for-byte the
same on every run and for every core count; a unit test compares one, two, and
eight workers. Cranelift's IR verifier checks the compiler's own output, not the
user's program, and runs only in debug builds of the compiler, which the tests
use. On the same machine (2026-10-10, release build, 16 hardware threads), full
AOT of 1,000 units / 39,003 lines fell from 3.25 s to 0.87 s, and emitting the
object for 5,000 units / 195,003 lines from 16.9 s to 4.35 s, of which checking
is 2.6 s. Peak memory is unchanged. On one core the second figure is 13.2 s:
that gain is the verifier alone.

Cranelift's single-pass register allocator was measured and not adopted. On
the 195,003-line program it cut emission CPU time from 18.5 s to 10.3 s but
wall time only from 4.4 s to 4.0 s, grew machine code by 37%, and slowed the
`examples/collection_bench.py` workloads by 3% to 20%. Plenty has one build
mode, and that trade does not suit it.
Functions without loan facts skip CFG loan analysis; no whole-program alias
analysis or per-call body inspection is required.
The legacy parser and explicit legacy entry points remain to exercise the
mature arithmetic, branch, ABI, tail-call, and runtime tests during migration.
There is no automatic syntax detection. These paths should be removed after
the new tests cover their useful backend behaviors.

The checker now builds an access CFG from typed operations, with explicit loop
backedges, branch successors, early exits, and stable binding-place IDs. Loan
liveness is solved to a fixed point before native lowering. A future unified typed
CFG with projected places and full source spans can replace the structured backend
input; the current restricted reference subset does not depend on that migration.
Borrow analysis never runs on Cranelift IR.
