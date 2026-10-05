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
| Concrete enums, tagged payloads, exhaustive matching | Implemented |
| Structs, associated methods | Planned |
| `Option[T]`, `Result[T, E]` | Implemented for stored value payloads |
| Value reclamation and affine generator moves | Implemented |
| Public references and general borrow checking | Planned |
| Interpreter, REPL, JIT | Out of scope |
| Lists, dictionaries, sets, ranges, `for`, comprehensions | Implemented |
| While loops, break/continue | Implemented |
| Lazy native `Generator[T]`, typed yield, consuming iteration | Implemented |
| User generics, traits | Deferred |
| Async/await | Out of scope |

Mutability checking and automatic reference counting are **not a borrow checker**.
Strings, collections, and current enums have independent-value semantics, using
immutable shared storage and automatic reclamation. Updating a collection replaces
one binding. Generators instead own advancing state: assignment, calls, returns,
and iteration move that state. A conservative definite-ownership pass rejects
use after a possible move. Public references and resource-bearing structs are
not implemented.

The four design proposals for this batch are in [docs/proposals](docs/proposals).
They record the reasoning and suggested staging; this document describes the
implemented result, including integration choices that differ from those proposals.

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
or parameters. Absence and recoverable failure use explicit sum types:
`Option[T].Some(value)` / `Option[T].Nothing` and
`Result[T, E].Ok(value)` / `Result[T, E].Err(error)`. Unit payloads, including
`Result[(), E]`, remain unsupported; a concrete enum with a nullary success
variant expresses that outcome.

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

`len`, `in`, and `not in` work with collections, strings, and ranges.
Generators support consuming iteration, not length or membership. Lists, strings, and
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
Managed values are reference counted. Replacing a local releases its previous
value; scope/function exits release remaining owners. Collection buffers and
type metadata are reclaimed along with objects. Private expression temporaries
can remain until their enclosing scope exits. Uniqueness-aware updates remain
an optimization to consider later.

String length is cached; scalar indexing scans UTF-8 boundaries. String iteration
uses a private byte cursor and is linear in byte length. Keys/values lists are snapshots. Hashes are not randomized. These are
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
This does not complete the source CFG migration needed for public borrow analysis.

Cranelift declares one SSA variable per slot; stores define
variables and joins use Cranelift's SSA construction. All names and types are
resolved before native emission. `PrintLine` formats modern values naturally,
and `FloorDiv` adds Python-compatible floor semantics without changing legacy
backend regressions. Strings use explicit byte/scalar lengths, including embedded
NUL; raw modern output writes exactly the stored byte length.

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

## One string type

There is one public `str`: an immutable sequence of Unicode scalar values.
Storage is valid UTF-8 with an explicit byte length and cached scalar count.
Embedded U+0000 is ordinary content; `\0` is a supported literal escape.
Neither literals nor dynamic strings have a trailing terminator. Equality and
hashing include every byte and do not normalize Unicode. `len` counts scalars,
not grapheme clusters; indexing (including negative indices) returns a one-scalar
`str`. Concatenation and indexing return independent values.

The native value is one pointer to a 32-byte prefix followed by exactly the UTF-8
payload: the 16-byte managed header, a u64 byte length, and a u64 scalar count.
Literal headers are aligned to eight bytes and immortal. There is no public
owning/view string distinction. Legacy input validates UTF-8, rejecting malformed
sequences and preserving embedded NUL. Future FFI adapters must explicitly
convert to pointer/length or temporary terminated C text; C-text export must
reject embedded NUL when the external API cannot represent it.

## Concrete enums and sum types

```python
enum Reading:
    Missing
    Value(i64)
    Invalid(str)

def describe(reading: Reading) -> str:
    match reading:
        case Reading.Missing:
            "missing"
        case Reading.Value(number):
            "positive" if number > 0 else "nonpositive"
        case Reading.Invalid(reason):
            reason
```

Enums are nominal module-level types. Variants have zero or more fixed positional
payloads; nullary variants omit parentheses. Qualified constructors and patterns
use an enum name, a transparent alias, or an explicit builtin instantiation such
as `Option[i64]`. Type/alias declarations may refer forward; recursive enum
dependencies (including through containers) are rejected initially. Enum names
share the type declaration namespace. A binding shadowing a type qualifier is
diagnosed rather than silently selecting different behavior.

Type nesting is limited to 64 levels, and expanded builtin type argument names
to 16,384 bytes, with diagnostics when these implementation limits are exceeded.
Runtime descriptors refer back to previously encoded enums, so shared enum
dependencies do not expand exponentially during compilation or metadata loading.

`Option[T]` and `Result[T, E]` are compiler-known concrete enum constructors,
without user generics or traits. All payloads must be stored, copyable values:
integers, bool, str, collections, or other nonrecursive enums. Unit and generator
payloads are rejected. Enums can be list elements and dictionary values, but are
not dictionary keys or set elements in the initial closed hashable-type set.

`match` currently accepts enums. Each `case` names a variant and binds payload
positions to immutable locals or `_`; a whole-value `_` covers the remaining
variants. Coverage is exhaustive and checked before lowering. Duplicate variants,
redundant wildcards, incorrect payload arity, and wrong enum identities are
errors. Nested patterns, guards, OR patterns, and scalar matching syntax are
deferred. The scrutinee is evaluated once. Arm bindings are scoped locally and
may shadow outer bindings. Continuing arms agree on result type; arms ending in
return/break/continue do not contribute a join value. A function-tail match
produces its final arm expression, like the existing statement-form `if`.

Native enum values are immutable pointer-sized handles to tagged records.
The record contains a managed header, concrete type metadata, tag, and one
64-bit slot per active payload field. Equality compares nominal type, tag, and
payload contents. Printing uses qualified variant names. No niche optimization,
stable external layout, or per-instantiation code generation is required.
Frontend coverage lowers to the existing scalar-tag match with an invalid-tag
trap fallback. The independent checker validates construction/projection types;
the structured frontend places projections behind the corresponding tag tests.

## Ownership and reclamation

Every managed expression operand, live local, stored field, and frame capture
has one owner. Loading a copyable value retains its immutable storage. Moving a
generator clears the source ownership slot. Stores evaluate the RHS, release the
old owner, then transfer the new one. Scope exits, loop exits, and function exits
release locals; compiler-private temporaries are bounded by local slots.
Tail-call arguments are owned before caller cleanup, preserving direct and mutual
tail calls. Traps terminate the process without unwinding language scopes.

Runtime objects share `{u64 refs, destroy_callback}`. Heap objects start with one
reference; literal strings use an immortal count. Helpers borrow arguments and
return owned managed results, including retained projections and builder aliases.
Buffers and recursive metadata have explicit owners too. Destruction uses an
iterative queue, avoiding recursive C-stack growth through owned value graphs.
Reference counts are non-atomic; the language has no concurrency. The current
immutable-value/affine-frame restrictions prevent source-visible ownership cycles.

Generators are the first affine values. The independent ownership pass tracks
definite availability of local slots through structured branches. Only continuing
arms join. A move on one branch makes the binding unavailable at a later join
unless reinitialized; exiting branches are excluded. Each loop backedge,
including `continue`, must preserve availability of outer owners available at
entry. A move followed by mutable reinitialization is accepted; a move reaching
a backedge is conservatively rejected. Break paths join the zero-iteration path.
No implicit generator copies or affine fields in copyable containers are allowed.

## Public borrowing — future work

Public references, resource-bearing structs, partial moves, and a general borrow
checker remain future work. Proposed references are ordinary `&T` and `&mut T`;
`str` remains the sole string value type. An exclusive reference to a string
binding would permit replacement, not arbitrary byte mutation. Before references,
introduce source-level places, a typed CFG, loan facts, and sound drop/borrow
checking. The current ARC and affine analysis do not provide those features.

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

## Native generators

A function containing `yield` declares `Generator[T]`. Calls evaluate arguments
and create an owned frame without running the body. Each resume executes native
code until a statement-only `yield value`, bare `return`, or fallthrough.
Yield types are exact and copyable; nested generator yield types and unit are
rejected. Generator functions cannot return a value. An ordinary factory without
`yield` may return another generator by moving it.

`next(g)` requires a named mutable generator binding and returns `Option[T]`.
This compiler-known operation borrows the owner only for the call. Exhaustion
is stable. `for`, comprehensions, and iterable collection constructors consume
generators; `break` destroys their hidden iterator owner without executing later
generator statements. Generator assignment, arguments, and returns transfer
ownership. Printing, equality, length, and membership are not defined on them.
There is no yield-from, send/throw, generator expression, public reference across
suspension, or async/await.

Each generator has a concrete constructor and native resume function. The frame
owns parameters and all locals in fixed 64-bit slots, plus a managed-slot mask,
resume callback, continuation state, and reentrancy guard. Resume has the internal
C ABI `(frame, out_slot) -> ready`; successful yields transfer an owned value.
Completion clears owned slots and marks exhaustion. Dropping any state frees
remaining captures without resuming the source body.

Integration deliberately reuses the checked structured operation tree instead
of introducing a second source IR in this batch. `Yield` requires an empty
residual operand stack. Native lowering adds resume-dispatch edges to continuation
blocks; all generator locals are frame-backed, so no SSA value needs to survive
between invocations. Cranelift verifies the resulting CFG. This is native
state-machine lowering, with no interpreter, C-stack suspension, or eager yield
collection. The initial state dispatch is a linear comparison chain.
Iteration currently wraps each resume result in an `Option`; avoiding that
allocation and optimizing frame liveness are later runtime improvements.

## Next milestones

1. First-class unit payloads, richer diagnostics, and measured compile-latency
   improvements; consider caching runtime compilation.
2. Concrete structs and methods; recursive types with explicit layout rules.
3. Typed source CFG/places and a sound local public-borrow subset, then evaluate
   Polonius-style precision against compilation cost.
4. Narrowly scoped generics if examples require them. Traits remain a separate
   decision. Async/await remains out of scope.

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
