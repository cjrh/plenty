# Plenty — language and compiler design

This document replaces the stack-language design. It is the current contract
for this branch. The [historical design](docs/legacy-design.md) remains useful
for backend implementation details; its language decisions are superseded.

## Direction

Plenty should feel familiar to a Python programmer while being a conventional,
statically typed, ahead-of-time compiled language. It is not Python compatible
and is not an attempt to reproduce Rust. Prefer a small language with explicit
interfaces over dynamic flexibility or elaborate compile-time machinery.

**Fast compilation is a primary goal**, including at the expense of language
features. Native execution, REPL feedback, simple semantics, and predictable
memory use matter. Cranelift is the native backend; work with its strengths.

Consequences:

- Every function parameter and return type is declared. Infer types within a
  function; never require whole-program inference to understand an interface.
- Compile each concrete function once. Begin monomorphic. Defer trait solving,
  specialization, implicit coercion searches, and user-defined compile-time
  execution. Avoid features whose analysis creates an unbounded search space.
- Keep parsing, type checking, ownership checking, and code generation separate.
  Cranelift receives already resolved operations with concrete types.
- Favor straight-line lowering and ordinary control-flow graphs. No dependency
  on LLVM, a Python interpreter, tracing, deoptimization, or runtime reflection.
- Optimize compiler simplicity and latency before adding expensive passes.
  Measure parse/check, lowering, native emission, and linking separately. The
  current AOT command recompiles the small embedded C runtime on each link;
  caching it is a future compile-latency improvement, not a measured result.
- No async/await, dynamic attributes, monkey-patching, metaclasses, inheritance,
  implicit nullable references, or exceptions in the initial language.

## Implementation status

| Area | Status on this branch |
| --- | --- |
| Python-shaped lexer/parser, typed function declarations | Implemented |
| Infix expressions, conditional expressions and blocks | Implemented |
| Immutable bindings and explicit `mut` reassignment | Implemented |
| Checked sized integers, booleans, strings, unit returns | Implemented |
| Cranelift AOT, interpreter, multiline REPL | Implemented |
| Direct and mutual tail calls | Implemented in interpreter and AOT |
| Structs, associated methods, tagged unions, exhaustive payload matching | Planned |
| `Option[T]`, `Result[T, E]` | Planned with sum types |
| Ownership, references, borrow checking | Planned; no current memory-safety claim |
| JIT | Not present in this checkout; future backend |
| Loops, early return, generators | Planned |
| User generics, traits | Deferred |
| Async/await | Out of scope |

Mutability checking alone is **not a borrow checker**. The current runtime
uses copyable scalar values and immutable, append-only strings. It does not
reclaim strings. Do not introduce references or owned aggregates before their
move/drop/borrow rules can be checked and lowered consistently.

## Current language contract

```python
def choose(flag: bool, first: int, second: int) -> int:
    """Choose one of two integers."""
    first if flag else second

def countdown(n: int) -> ():
    if n == 0:
        pass
    else:
        print(n)
        countdown(n - 1)

mut answer: int = choose(True, 40, 0)
answer = answer + 2
print(answer)
```

### Syntax and values

Spaces delimit indented suites; tabs are rejected. Indentation must return to
an existing indentation level. Blank/comment lines do not affect indentation.
Parentheses permit multiline expressions. Comments begin with `#`. Identifiers
are ASCII letters, digits, and underscores, and cannot start with a digit.
Names beginning `__plenty_` are reserved for compiler-generated functions.
Parser diagnostics carry one-based line and column positions.

Use Python spellings `def`, `True`, `False`, `and`, `or`, `not`, and `pass`.
Strings may use single, double, or triple quotes; triple quotes permit physical
newlines. The first standalone string in a function is its documentation.
To return a string directly without a docstring, use `return "text"`.

The current primitive types are `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`,
`u64`, `bool`, and `str`. `int` is exactly `i64`, not a Python arbitrary-precision
integer or a target-dependent word. Unsuffixed literals are `i64`; suffixes
select widths. No contextual integer inference or implicit numeric widening.
Integer casts use truncation/sign-extension like the historical backend.
Overflow and division by zero are runtime errors in both execution paths.
`//` floors signed quotients, including negative operands; `/` is rejected
until a floating-point type is implemented. Comparisons require equal types,
and ordering requires integers. Chained comparisons are rejected explicitly.

There is **no `None` type or value**. The unit type `()` means a computation
completed without producing data, and lowers to no result register. It is
currently supported for expressions and function returns, not stored bindings
or parameters. Absence and recoverable failure will be explicit sum types.
`Option[T]` may have an `Absent` case; that case is not implicitly coercible
to any other type. `Result[T, E]` distinguishes success from failure and does
not replace `Option[T]` or unit.

### Functions, expressions, and control flow

All declarations are top-level, with complete signatures. There are no nested
functions, closures, default/keyword arguments, overloads, or redefinitions.
Signatures are collected before any body is checked, allowing forward calls
and mutual recursion. Top-level definitions are installed before executable
statements run, in both execution modes.

A suite's last expression is its value. Both sides of a value-producing
`if`/`elif`/`else` must agree. A non-final expression is evaluated and discarded;
a non-final conditional discards its branches' results. An `if` without an
`else` can only have unit result. The inline conditional uses Python order:
`value_if_true if condition else value_if_false`.

Conditions and Boolean operators accept only `bool`; there is no truthiness.
`and` and `or` short-circuit. Arguments and ordinary binary operands evaluate
left to right. Each expression is evaluated once.

`return` is currently allowed only at the end of a function suite or a branch
in its final conditional. Early return, loops, break/continue, and arbitrary
control-flow joins need the next IR milestone; they are rejected rather than
silently compiled with different behavior. Tail calls in final expressions
and final branches are rewritten to tail-call operations. The interpreter
replaces its call frame and Cranelift emits `return_call` with the Tail calling
convention. Ordinary nested calls retain normal call semantics.

### Bindings and mutation

`name = expression` first declares an immutable local with inferred type.
`name: type = expression` provides an explicit type. `mut name = expression`
or `mut name: type = expression` declares a mutable local. Later `name = value`
assigns to an existing local and requires `mut` and the same type. Repeating
`mut` or a type annotation on an existing name is a duplicate declaration.
Parameters are immutable. Initializers cannot read their own new binding.

Branch-local declarations do not escape their branch. Assignments to existing
mutable locals do persist across branch joins. There are no uninitialized
declarations. Parameters plus locals are currently limited to 256 slots per
function, a checked implementation limit inherited from the compact IR.

Top-level bindings are locals of a generated entry function. In the REPL,
functions persist, but bindings currently last only for one submission.
Separate submissions do not consume earlier expression results. Compilation
and type errors execute nothing and preserve prior definitions/results.
Runtime errors can leave effects that have already occurred. Function
redefinition is rejected to keep previously checked callers valid.

## Compiler architecture

```text
source → indentation lexer → AST → signature collection → local type checking
                                                        ↓
                                              typed operation IR
                                               ↙             ↘
                                         interpreter      Cranelift → object → cc
```

`frontend.rs` owns modern syntax, name resolution, local inference, mutability,
and lowering. It emits operations directly, never translated legacy source.
`op.rs` remains a backend-neutral operation IR and an independent type checker.
Its compile-time operand stack is an implementation detail, not a language
feature. `CompiledFn` carries a signature, documentation, body, and local-slot
types. `LoadLocal`/`StoreLocal` address typed slots; conditional expressions
use exhaustive Boolean branches. Unit is represented by zero stack values.

The interpreter allocates local slots per call and releases the frame on
return/tail call. Cranelift declares one SSA variable per slot; stores define
variables and joins use Cranelift's SSA construction. All names and types are
resolved before native emission. `PrintLine` formats modern values naturally,
and `FloorDiv` adds Python-compatible floor semantics without changing legacy
backend regressions. Strings reject NUL because the inherited AOT runtime uses
C strings. UTF-8 strings otherwise work with raw modern output.

The public `Vm::run`/`eval` and `compile_source_to_executable` use modern syntax.
`check_source` and `--check` validate without execution or native emission.
`examples/compile_bench.rs` generates a repeatable function workload and reports
median checking time, optionally including full AOT compilation and linking.
It reports build mode, target, source size, function count, and repetitions;
record machine details alongside any published result.

Initial diagnostic baseline (2026-10-05): AMD Ryzen 7 7840HS, x86_64 Linux,
debug Rust build, 100 generated functions / 6,194 source bytes, one warm-up and
five measured repetitions. Median parse/resolve/check was 1.112 ms; full AOT
including checking, native emission, C runtime compilation, and linking was
47.275 ms. This small synthetic workload is a starting measurement, not a
release-performance guarantee or a bound for larger programs.
The legacy parser and explicit legacy entry points remain to exercise the
mature arithmetic, branch, ABI, tail-call, and runtime tests during migration.
There is no automatic syntax detection. These paths should be removed after
the new tests cover their useful backend behaviors.

Before implementing references, replace nested control-flow operations with
a small typed CFG IR: basic blocks, explicit terminators, stable local/place
IDs, source spans, and uses/definitions. Add definite-initialization, moves,
drops, and loan facts there. Both Cranelift AOT and a future JIT should consume
the same checked CFG. Do not run borrow analysis on Cranelift IR: source-level
ownership and place information would already have been lost.

## Structs and sum types — proposed next milestone

Use `struct`, with named, statically typed fields and associated methods.
There is no class hierarchy, object dictionary, implicit boxing, or automatic
dynamic dispatch. A possible syntax (not accepted yet):

```python
struct Point:
    x: i64
    y: i64

    def length_squared(self: &Point) -> i64:
        self.x * self.x + self.y * self.y

enum Reading:
    Missing
    Value(i64)
    Invalid(str)
```

Decide aggregate layout and calling conventions explicitly. Begin with tagged
unions (tag plus payload) and exhaustive `match`/`case`; postpone niche layout
optimization. Shared layouts must work in the interpreter and AOT/JIT. Struct
construction must initialize every field. Pattern coverage is checked on the
known finite variant set; payload binding is statically typed.

Implement concrete structs and enums before general generics. `Option[T]`
and `Result[T, E]` may initially be compiler-known type constructors with
straightforward per-concrete-type layouts. This is an explicit bootstrap
decision, not a user-extensible trait system. Unconstrained parametric functions
and generic containers do not inherently require Rust's trait machinery;
operation-constrained generics need a separately designed capability mechanism.
Defer that mechanism until real examples justify it, and budget specialization
costs before permitting it.

## Ownership and borrowing — design target

Use value ownership and moves for owned aggregates, immutable bindings by
default, shared read-only borrows `&T`, and exclusive mutable borrows `&mut T`.
Primitive scalars are copyable. Do not automatically copy arbitrary aggregates.
Specify strings' owning/view types before replacing the current string arena.
Mutability belongs to a binding/place; exclusivity belongs to a loan.

Prefer last-use/flow-sensitive loan checking over lexical-lifetime rules.
Polonius is the relevant Rust work: it models relationships between reference
origins and loans over control flow. Rust's Polonius alpha was enabled on
nightly in August 2026; stabilization and formal modeling remain work items.
The old standalone Datalog engine is not automatically the current rustc
implementation. We should reuse concepts and test cases, not assume that
adding a crate supplies a sound checker for Plenty.

Start with local borrows and no returned/stored references. Once source places,
aliasing, moves, reborrows, joins, and drop points are modeled, add a restricted
reference-return rule whose origin is unambiguous from the signature. Reject
ambiguous cases before introducing lifetime syntax. Never infer cross-function
borrowing contracts by inspecting callee bodies. A small sound subset is
preferable to a permissive checker with gaps.

Correctness gates include use-after-move, conflicting shared/exclusive loans,
mutation during a live shared borrow, branch-dependent loans, reborrows,
returning local references, partial moves, and ownership across control-flow
joins. No memory-safety claim until these rules and destruction are implemented.

Sources informing this design:

- [Polonius alpha nightly announcement](https://blog.rust-lang.org/2026/08/04/enabling-polonius-alpha-on-nightly/)
- [2026 Polonius stabilization/modeling goal](https://goals.rust-lang.org/2026/polonius.html)
- [Borrow checker roadmap](https://goals.rust-lang.org/2026/roadmap-borrow-checker-within.html)

## Generators and later work

Generators are desirable; async/await remains out of scope. Lower a generator
to an explicit state machine with a concrete frame type and a resume operation
returning `Option[T]`. Initially allow only owned yields and prohibit borrows
across suspension. Destruction must handle every suspension state. This needs
sum types, ownership/drop semantics, and CFG analysis first, but does not need
a trait system. A compiler-known `Generator[T]`/iteration protocol can come first.

Tail-call optimization is already retained. Keep its calling-convention
constraints explicit as references and destructors arrive: a pending drop or
a borrow of the caller's frame can prevent frame replacement. Do not promise
arbitrary calls become tail calls merely because they occur near a return.

Implementation order:

1. Current typed-expression vertical slice, native parity, and compile-latency
   measurement harness. Establish useful small/large-program baselines.
2. Typed CFG, loops and early returns, richer source diagnostics, persistent
   REPL bindings, and shared module-independent backend lowering.
3. Concrete structs, methods, enums, exhaustive matching, `Option`/`Result`.
4. Ownership, destruction, and a sound local borrow subset; then evaluate
   Polonius-style precision versus compile-time cost on real Plenty programs.
5. Cranelift JIT with compatible runtime ownership and session symbol rules.
6. Owned generators, then narrowly scoped generics if needed. Traits remain
   an independent decision, not a prerequisite imposed on the first language.

Tests must distinguish proposed syntax from executable examples. Every current
example should run. Native parity tests exercise output, errors, side-effect
ordering, branch-local mutation, integer widths, and deep tail recursion.
Performance results must name the build mode, machine, input size, and whether
linking/runtime compilation is included; no latency claim without measurement.
