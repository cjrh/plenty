# Plenty — language and compiler design

This document replaces the stack-language design. It is the current contract
for this branch. The [historical design](docs/legacy-design.md) remains useful
for backend implementation details; its language decisions are superseded.

[TUTORIAL.md](TUTORIAL.md) is the learner-facing companion. Build it alongside
the implementation: teaching a feature and addressing awkward examples is part
of designing that feature. The tutorial teaches implemented behavior; this
document also records future design decisions.

## Direction

Plenty should feel familiar to a Python programmer while being a conventional,
statically typed, ahead-of-time compiled language. It is not Python compatible
and is not an attempt to reproduce Rust. Prefer a small language with explicit
interfaces over dynamic flexibility or elaborate compile-time machinery.

**Fast compilation is a primary goal**, including at the expense of language
features. Native execution, quick edit/check/run cycles, simple semantics, and predictable
memory use matter. Cranelift is the native backend; work with its strengths.

Plenty supports **AOT only**. There is no interpreter or REPL, and JIT support
is out of scope. Language features and runtime layouts target native compilation
without an obligation to support a second execution engine.

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
| Transparent module-level type aliases | Implemented |
| Cranelift AOT and compile-and-run file command | Implemented |
| Direct and mutual tail calls | Implemented in AOT |
| Early returns and return-aware branch checking | Implemented in AOT |
| Structs, associated methods, tagged unions, exhaustive payload matching | Planned |
| `Option[T]`, `Result[T, E]` | Planned with sum types |
| Ownership, references, borrow checking | Planned; no current memory-safety claim |
| Interpreter, REPL, JIT | Out of scope |
| Lists, dictionaries, sets, ranges, `for`, comprehensions | Implemented |
| While loops, break/continue | Implemented |
| Generators | Planned; separate state-machine milestone |
| User generics, traits | Deferred |
| Async/await | Out of scope |

Mutability checking alone is **not a borrow checker**. The current runtime
uses copyable scalars, immutable strings, and immutable collection values. A
collection update replaces a mutable binding with a new value; no mutable
references or shared mutable contents are exposed. Collections are a deliberate
value-semantic bridge to the ownership work, not evidence that a borrow checker
exists. Allocations are retained until process exit; owned resources and references
still require move/drop/borrow checking.

## Current language contract

```python
def choose(flag: bool, first: i64, second: i64) -> i64:
    """Choose one of two integers."""
    first if flag else second

def countdown(n: i64) -> ():
    if n == 0:
        pass
    else:
        print(n)
        countdown(n - 1)

mut answer: i64 = choose(True, 40, 0)
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
`u64`, `bool`, and `str`. Numeric built-ins use explicit-width names; there is
no built-in `int`. Future floating-point types should use `f32`/`f64`; neither
is implemented yet. Unsuffixed integer literals are `i64`; suffixes
select widths. No contextual integer inference or implicit numeric widening.
Integer casts use truncation/sign-extension like the historical backend.
Overflow and division by zero are runtime errors.
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

### Type aliases

```python
type int = i32
type Count = int

def increment(value: Count) -> Count:
    value + Count(1)
```

`type Name = Type` declares a transparent, non-generic alias at module scope.
Aliases may target any supported primitive, collection type, `range`, `()`, or another alias. They are
visible throughout the module and may refer to later declarations. Cycles,
unknown targets, duplicate aliases, and collisions with built-ins or function
names are rejected, including unused aliases. Local aliases are not supported.
An alias creates no new nominal identity, validation rule, layout, or runtime
wrapper; two aliases for `i32` are interchangeable with each other and `i32`.

Use aliases in parameter/return types and local annotations. Integer aliases
also support the same explicit cast syntax as their target: with `type int =
i32`, `int(42)` is exactly `i32(42)`, including truncation semantics. Boolean,
string, and unit aliases do not introduce constructors or conversions. Existing
restrictions on storing or passing unit still apply through aliases.

Aliases never change literal defaults or add literal suffixes. With the alias
above, `x: int = 42` fails because the unsuffixed literal is still `i64`; write
`x: int = 42i32` or `x: int = int(42)`. `42int` is not a valid suffix. Current
examples use explicit numeric widths unless they are teaching aliases.

Aliases belong to their source module. Type lookup uses a separate namespace
from local bindings; a local may shadow a callable cast name without changing
the meaning of type annotations.

### Functions, expressions, and control flow

All declarations are top-level, with complete signatures. There are no nested
functions, closures, default/keyword arguments, overloads, or redefinitions.
Signatures are collected before any body is checked, allowing forward calls
and mutual recursion. All declarations and statements are checked before any
native code is emitted or executed.

A suite's last expression is its value. Continuing branches of a value-producing
`if`/`elif`/`else` must agree. A non-final expression is evaluated and discarded;
a non-final conditional discards its branches' results. An `if` without an
`else` can only have unit result. The inline conditional uses Python order:
`value_if_true if condition else value_if_false`.

Conditions and Boolean operators accept only `bool`; there is no truthiness.
`and` and `or` short-circuit. Arguments and ordinary binary operands evaluate
left to right. Each expression is evaluated once.

`return value` exits the enclosing function from any suite; bare `return`
returns unit. Every explicit return is checked against the declared return
type, even inside a non-final or statically unchosen branch. A returning branch
does not participate in the type join of paths that continue. A function with
any continuing path must still produce its declared result on that path; an
`if` without `else` cannot prove that all paths return. This analysis is
structural, without constant-condition folding. Statements after an explicit
return or an exhaustive conditional whose branches all return are rejected
as unreachable at their source position.

```python
def clamp_low(value: i64, minimum: i64) -> i64:
    if value < minimum:
        return minimum
    value
```

`for` and `while` loops, including `break` and `continue`, are implemented.
Tail calls in final expressions, final branches, and explicit return
expressions (including early guard clauses) become tail-call operations.
Cranelift emits `return_call` with the Tail calling
convention. Ordinary nested calls retain normal call semantics.

### Collections and iteration

The compiler-known constructors `list[T]`, `set[T]`, and `dict[K, V]` accept
concrete element types, including nested collections. Dictionary keys and set
elements are restricted to integers, `bool`, and `str`; there is no user-defined
hash/equality protocol. Unit elements are rejected. These built-ins do not expose
general user generics or require a trait solver.

Literals use Python spelling: `[1, 2]`, `{"a": 1}`, and `{1, 2}`. Elements must
have exactly the same type. Empty literals need context from an annotation,
parameter, or return type; typed constructors such as `list[i64]()`,
`dict[str, i64]()`, and `set[i64]()` also work. `{}` always means dictionary.
Nonempty literals infer their type from the first element; integer literals still
default to i64 without implicit narrowing. Aliases may name collection types.

Collections have independent-value semantics. Assignment and function arguments
may share immutable storage. `xs.append(value)`, `members.add(value)`, and
`mapping[key] = value` replace a named `mut` binding; `xs[index] = value` does
likewise. Parameters remain immutable. Mutating one binding does not change
another binding, an existing nested collection, or an iteration already in
progress. Nested indexed mutation is not implemented: extract the inner value,
update a mutable binding, then assign that value back to its parent explicitly.

Lists preserve order and duplicates. Dictionaries preserve first insertion order;
a repeated key replaces its value without moving the key. Sets remove duplicates
and promise no iteration order. Equality is structural: list order matters;
dictionary insertion order and set order do not. There are no identity tests.

`len`, `in`, and `not in` work with built-in iterables. Lists, strings, and
ranges support i64 indexing, including negative indices. Dictionaries index by
their key type. Out-of-bounds indices and absent keys report a runtime error and
exit with status 1; absence-returning dictionary lookup awaits `Option`.
`list(iterable)` and `set(iterable)` convert supported iterables; `dict(d)`
copies an existing dictionary value. Dictionary `keys()` and `values()` produce
snapshot lists, not mutable views. Pair iterables and `items()` await tuples.

`range(stop)`, `range(start, stop)`, and `range(start, stop, step)` use i64
arguments, exclude stop, and store only start/stop/step/length. A zero step or
length exceeding i64 is a runtime error. Negative steps and extreme i64 bounds
are checked using wider intermediate arithmetic. Range membership is constant
time. `%` uses the divisor's sign, like Python; division by zero is an error,
while `INT_MIN % -1` is zero.

`for name in iterable:` evaluates its iterable once and iterates a snapshot:
lists yield elements, dictionaries keys, sets elements, ranges integers, and
strings Unicode scalar values as `str`. Loop variables and declarations are
block-local and immutable; updates to enclosing mutable bindings persist. Even
a body that always returns cannot prove a loop executes, so function return
checking retains the zero-iteration path. Loops have unit value.

`while condition:` checks a Boolean condition before each iteration. Its body
has the same binding scope as a `for` body. `break` exits the innermost loop;
`continue` skips the rest of that iteration. In a `for`, continuing advances
the iterator exactly once; in a `while`, it reevaluates the condition. Neither
accepts a value or may cross a function boundary. Statements after an
unconditional control-flow exit are rejected. Loop `else` clauses are not
supported. Return checking conservatively retains a fallthrough path even for
`while True`; value-returning functions need a result after the loop.

List, set, and dictionary comprehensions accept multiple `for` and `if`
clauses. Clauses nest left to right; each iterable is evaluated when its enclosing
iteration reaches it. Filters run before the result expression. Dictionary keys
are evaluated before their values. Comprehension variables have their own scope,
and the first iterable sees the enclosing scope. Conditional expressions in
iterables or filters must be parenthesized. No generator expressions, async
iteration, tuple unpacking, arbitrary iterator protocol, or user special methods.

Python's [display and comprehension rules](https://docs.python.org/3/reference/expressions.html#displays-for-lists-sets-and-dictionaries)
inform evaluation order and scope; [dictionary semantics](https://docs.python.org/3/library/stdtypes.html#mapping-types-dict)
inform key iteration, replacement, and insertion order. Independent values,
block-local loop variables, and fixed element types are deliberate Plenty choices.

#### Collection implementation and current costs

`collection.rs` gives each operation one static signature. The frontend lowers
loops and comprehensions into typed locals and structured loops. Cranelift calls
a fixed native collection ABI with 64-bit slots; descriptors encode the already
known element types. Descriptors support native storage/equality/printing, not
dynamic type inference. No per-element compilation or trait instantiation occurs.
Programs using only scalar operations do not compile or link the collection
runtime; its C source is included only when operations or signatures require it.

The C runtime uses growable list storage and hash tables with ordered entries for
dictionaries and sets. Private builders append in place; literal and comprehension
construction is amortized linear under ordinary hash distribution. Public updates
copy the outer storage, so repeated `append` updates can be quadratic; prefer a
comprehension for bulk construction. Nested immutable values are shared safely.
Collection allocations are tracked and freed at process exit, not at last use.
Long-running allocation-heavy programs therefore retain memory. Earlier scalar
string helpers still have their append-only allocation policy. Ownership-based
reclamation and uniqueness-aware updates are later work.

String length and indexed access scan UTF-8 text, so string iteration is currently
quadratic. Keys/values lists are snapshots. Hashes are not randomized. These are
explicit initial runtime limits, not promises of Python's complete container API.
Compiler-generated builder and iterator locals count toward the 256-slot limit.

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

Top-level bindings are locals of a generated entry function. A module's final
value is discarded; use `print` for observable output. Compilation and type
errors execute nothing. Runtime errors can leave effects that have already
occurred.

## Execution commands

- `plenty FILE` compiles into a private temporary directory, executes the native
  binary, then removes the directory. The child inherits standard input/output,
  the environment, and the current working directory. Its exit code is propagated;
  on Unix, signal termination is reported as 128 plus the signal number.
- `plenty --compile FILE -o OUT` produces a standalone executable.
- `plenty --check FILE` validates without native emission, linking, or execution.
- `plenty` with no arguments displays help.

Both execution commands use the same compiler and embedded runtime. Running
and compiling require a system C compiler named `cc`; checking does not.
Temporary object/runtime files are also removed after success or failure.
Compile-and-run adds compilation and linking to startup time; benchmark both
as part of the edit/run workflow.

## Compiler architecture

```text
source → lexer → AST → alias resolution → signatures → local type checking
                                                       ↓
                                              typed operation IR
                                                       ↓
                                             Cranelift → object → cc → executable
```

`frontend.rs` owns modern syntax, name resolution, local inference, mutability,
and lowering. It emits operations directly, never translated legacy source.
The parser retains type references with source positions. Before collecting
signatures, the frontend resolves aliases with an iterative dependency walk,
including references inside collection arguments, and caches each concrete
result. Cycles are rejected without Rust recursion on long alias chains. All signatures and annotations normalize to existing `Ty`
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
This does not complete the source CFG migration needed for ownership analysis.

Cranelift declares one SSA variable per slot; stores define
variables and joins use Cranelift's SSA construction. All names and types are
resolved before native emission. `PrintLine` formats modern values naturally,
and `FloorDiv` adds Python-compatible floor semantics without changing legacy
backend regressions. Strings reject NUL because the inherited AOT runtime uses
C strings. UTF-8 strings otherwise work with raw modern output.

The public `compile_source_to_executable` uses modern syntax.
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
drops, and loan facts there. Cranelift AOT should consume the checked CFG. Do not run borrow analysis on Cranelift IR: source-level
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
optimization. Layouts must support Cranelift's native calling conventions. Struct
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
Primitive scalars are copyable. Built-in collections have explicit independent-value
semantics, implemented with immutable sharing and replacement updates. This does
not decide the copying rules for arbitrary structs or resource-owning aggregates.
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

Generators are a separate milestone from ordinary loop control: a loop runs
within one function invocation, whereas a generator preserves locals and its
execution position between calls. Async/await remains out of scope. Lower a generator
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

1. Current typed-expression vertical slice, native execution tests, and compile-latency
   measurement harness. Establish useful small/large-program baselines.
2. Typed CFG, richer source diagnostics, and module-independent
   native lowering.
3. Concrete structs, methods, enums, exhaustive matching, `Option`/`Result`.
4. Ownership, destruction, and a sound local borrow subset; then evaluate
   Polonius-style precision versus compile-time cost on real Plenty programs.
5. Owned generators, then narrowly scoped generics if needed. Traits remain
   an independent decision, not a prerequisite imposed on the first language.

Tests must distinguish proposed syntax from executable examples. Every current
example should run. Native execution tests exercise output, errors, side-effect
ordering, branch-local mutation, integer widths, and deep tail recursion.
Early-return tests also cover guard fallthrough, all-path returns, mixed
explicit/implicit results, unit returns, nested-frame cleanup, skipped side
effects/errors, unreachable code, and explicit direct/mutual tail calls.
`tests/test_tutorial.rs` reads `TUTORIAL.md` directly: every `plenty` fence must
have a following `output` fence and runs through compile-and-run and an explicitly
compiled binary; every `plenty-error` fence has an `error` substring and must fail
in both commands
without executing effects. Do not maintain a separate copy of tutorial source
in tests. Update the guide as part of each learner-visible language change.
The stack-language tutorial is an unmaintained historical archive; native legacy
regression tests specify their expected output independently.
Performance results must name the build mode, machine, input size, and whether
linking/runtime compilation is included; no latency claim without measurement.
