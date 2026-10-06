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

- Every function parameter and return type is declared (a method's class supplies
  the type of its bare `self` receiver). Infer types within a
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
  runtime is compiled once when building Plenty and embedded as a native static
  archive. Each AOT build extracts the archive and links it with the generated object.
- No async/await, dynamic attributes, monkey-patching, metaclasses, inheritance,
  implicit nullable references, or exceptions in the initial language.

## Implementation status

The core language can compile substantial single-file programs: typed functions,
control flow, collections, classes, sum types, generators, ownership, and automatic
cleanup are implemented. It is still an early language implementation, with a
small built-in library and important limits on borrowing. The main gaps for
everyday programs are input and file APIs,
and richer text/collection operations. An implemented row below describes the
supported subset, not Python's full API or Rust's full ownership system.

| Area | Status on this branch |
| --- | --- |
| Python-shaped lexer/parser, typed function declarations | Implemented |
| Infix expressions, conditional expressions and blocks | Implemented |
| Immutable bindings and explicit `mut` reassignment | Implemented |
| Checked sized integers, booleans, strings, unit returns | Implemented |
| Floating-point types and arithmetic (`f32`, `f64`, `/`) | Implemented with IEEE arithmetic and explicit numeric casts |
| Transparent module-level type aliases | Implemented |
| Cranelift AOT and compile-and-run file command | Implemented |
| Explicit binary `main` entry point | Implemented: parameterless `main` returns `()` or an `i32` process status; module scope contains declarations and imports |
| Rust runtime, embedded precompiled archive | Implemented; runtime compilation happens when building Plenty |
| Direct and mutual tail calls | Implemented where borrowing and observable cleanup permit |
| Early returns and return-aware branch checking | Implemented in AOT |
| Concrete enums, tagged payloads, exhaustive matching | Implemented |
| Fixed-layout classes, constructors, methods, custom cleanup | Implemented |
| `Option[T]`, `Result[T, E]` | Implemented with allocation-free inline wrappers, unit payloads, and unqualified `Some`, `Nothing`, `Ok`, `Err` |
| Unit values | Expressions, function returns, and enum payloads implemented; standalone bindings, parameters, and collection/class storage deferred |
| Value reclamation, owned moves, explicit copy/drop | Implemented |
| Local/parameter references and last-use borrow checking | Implemented for bindings and class fields; element/stored/returned references deferred |
| Interpreter, REPL, JIT | Out of scope |
| Lists, dictionaries, sets, ranges, `for`, comprehensions | Implemented |
| Borrowed collection iteration | Copyable elements only; borrowing owned elements is not implemented |
| Collection convenience APIs | Basic indexing, membership, append/add, updates, keys/values, optional list/dictionary `get`, list/dictionary `pop`, set `discard`, and fallible forward list slices; slice syntax and steps are deferred |
| Text convenience APIs | Length, indexing, iteration, concatenation, equality, membership, fallible joining, forward slicing, literal replacement, and explicit-separator splitting; formatting and numeric parsing are missing |
| Tuples, unpacking, dictionary `items()` | Not implemented |
| Allocation-free text queries | `startswith`/`endswith` return `bool`; `find`/`rfind` return optional scalar positions; `count` returns non-overlapping occurrence counts |
| Recoverable text trimming | `try_strip`, `try_lstrip`, and `try_rstrip` remove Unicode whitespace at selected ends |
| Recoverable text repetition | `try_repeat(i64)` creates repeated UTF-8 with checked lengths and one output allocation |
| In-place collection utilities | `list.reverse()` reorders elements; list/dictionary/set `clear()` drops contents while retaining capacity |
| Recoverable bulk collection mutation | `list.try_extend(list)` and dictionary/set `try_update` consume same-typed sources and reserve before changing contents |
| Set relationships | `issubset`, `issuperset`, and `isdisjoint` observe same-typed sets without allocating |
| Fallible set algebra | `try_union`, `try_intersection`, `try_difference`, and `try_symmetric_difference` return independent sets and preserve both same-typed inputs |
| While loops, break/continue | Implemented |
| Lazy native `Generator[T]`, typed yield, consuming iteration | Implemented |
| Absolute module imports and `pub` visibility | Implemented: one source root, private-by-default declarations/members, qualified imports and aliases; cycles and re-exports deferred |
| Modern program input, file I/O, and command-line argument APIs | Not implemented; modern programs currently expose output through `print` |
| Recursive class/enum types | Not implemented; acyclic forward declarations work |
| Native FFI / shared-library loading | Not implemented |
| User generics and structural protocols | Proposed; no user generics or protocol checking implemented yet |
| Typed ranges and contextual numeric inference | Proposed: `range[u8](8)` and expression-local constraints from annotations; ranges currently yield `i64` |
| Anonymous functions and closures | Proposed future work, including multiline bodies and checked capture ownership |
| `?` error propagation | Implemented for `Result` and `Option`, with matching error types and automatic early-exit cleanup |
| `with` context managers | Proposed; automatic destruction works today |
| Recoverable allocation failure | Collection `try_new`/`try_with_capacity` constructors and `try_reserve`/`try_append`/`try_add`/`try_insert` methods return `Result` with allocation-free `AllocError`; other allocating operations remain terminal on failure |
| Recoverable duplication | `try_copy(value)` returns `Result[T, AllocError]`, preserving the source and reclaiming partial copies on failure |
| Recoverable dictionary snapshots | `try_keys()` and `try_values()` return `Result[list[T], AllocError]` in insertion order, with no implicit deep copy |
| Recoverable text operations | `str.try_concat(other)`, `str.try_join(parts)`, `str.try_slice(start, stop)`, and `str.try_replace(old, new)` return `Result[str, AllocError]`; `str.try_split(separator)` returns `Result[list[str], AllocError]` |
| Checked text lookup | `str.try_get(index)` returns `Result[Option[str], AllocError]`; missing indices allocate nothing |
| Custom allocators and allocator provenance | Proposed; runtime storage still uses Rust's fixed global allocator |
| Threads, channels, parallel loops, SIMD | Proposed future work; current runtime is single-threaded |
| Standalone lesson sources and generated tutorial | Proposed; current Markdown examples already run in tests |
| Async/await | Out of scope |

Collections, classes, generators, and enums containing owned values transfer ownership.
`copy(value)` explicitly duplicates mutable contents; `drop(value)` consumes an
owner early. Immutable strings and immutable enums may share storage. Collection
updates operate in place. Named local and parameter references use `&T` / `&mut T`,
with last-use loan checking over an access CFG. Class fields can also be borrowed;
collection element references, stored references, and returned references are deferred.

The original four feature proposals are in [docs/proposals](docs/proposals).
They record the reasoning and suggested staging; this document describes the
implemented result, including integration choices that differ from those proposals.

The [next-phase design review](docs/proposals/next-language-phase.md) captures the
new entrypoint, module, FFI, protocol, memory, parallelism, SIMD, and documentation
directions. Its syntax is provisional. Rows marked proposed above are not usable
language features; the current contract below continues to describe implemented
behavior.

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

def main() -> ():
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
no built-in `int`. Floating-point types are `f32` and `f64`. Unsuffixed integer
literals are `i64`; decimal/exponent literals default to `f64`. Suffixes select
widths, including `1f32`, `1.5f32`, and `1e-3f64`. Decimal forms such as `.5`
and `1.` are also accepted.
No contextual numeric literal inference or implicit numeric widening.

The [typed-range and inference proposal](docs/proposals/typed-ranges-and-expression-inference.md)
recommends square-bracket function type arguments, deferred literal defaults,
and context flowing through one initializer or return expression. It also records
integer-family constraints, unsigned range boundary choices, and requirements
for future multiline closures. These are design proposals; the numeric rules
in this section continue to describe implemented behavior.

Integer casts use truncation/sign-extension like the historical backend.
Integer overflow and division by zero are runtime errors. `//` floors signed
integer quotients, including negative operands; `%` is also integer-only.
`/` accepts same-width floats. Floating-point arithmetic uses IEEE semantics:
signed zero, infinities, NaN, and ordinary rounding, without fast-math rewrites.
Float literals must fit their width; runtime arithmetic can overflow to infinity.
Underflow follows the target's IEEE behavior. Float-to-integer casts truncate
toward zero and saturate to the target range; NaN becomes zero. Integer-to-float
casts and narrowing floats may round; widening `f32` to `f64` is exact.
Comparisons require equal types; ordering accepts integers and floats. NaN
compares unequal to every value, including itself, and all ordered comparisons
with NaN are false. These rules also apply to structural equality and membership.
Float printing uses shortest round-trip Rust debug formatting, including a decimal
point for whole finite values and `inf`, `-inf`, and `NaN`.
Floats cannot be dictionary keys or set elements. Chained comparisons remain rejected.

There is **no `None` type or value**. The unit type `()` means a computation
completed without producing data, and lowers to no result register. It is
supported for expressions, function returns, and enum payloads, including
`Result[(), E]` and `Option[()]`. Standalone unit bindings, parameters, collection
elements, and class fields remain unsupported. Absence and recoverable failure
use explicit sum types: `Some(value)` / `Nothing` and `Ok(value)` / `Err(error)`.
`Ok(())` means success without data, not absence.

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

Use aliases in parameter/return types and local annotations. Numeric aliases
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
convention. Functions with resource-bearing parameters or locals retain ordinary
calls so observable cleanup happens after the callee returns.

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

Assignment, owned arguments, and returns move collections. Independent duplication
requires `copy(value)`, which recursively copies mutable contents while retaining
immutable strings and enums. `append`, `add`, and indexed updates mutate in place
through a named `mut` owner or an exclusive reference. Mutation arguments and
indices are evaluated before exclusive access to the target is taken, supporting
`xs.append(len(xs))` and `xs[len(xs) - 1] = value` without two-phase loans.
Nested indexed mutation and element references are deferred: use
`mut child = copy(parent[index])`, update it, and transfer it back to the parent.
Owned values cannot be moved directly out of indexed storage.

Lists preserve order and duplicates. Dictionaries preserve first insertion order;
a repeated key replaces its value without moving the key. Sets remove duplicates
and promise no iteration order. Equality is structural: list order matters;
dictionary insertion order and set order do not. There are no identity tests.

`len`, `in`, and `not in` work with collections, strings, and ranges.
Generators support consuming iteration, not length or membership. Lists, strings, and
ranges support i64 indexing, including negative indices. Dictionaries index by
their key type. Out-of-bounds indices and absent keys report a runtime error and
exit with status 1. `dictionary.get(key)` returns `Option[V]`: `Some(value)`
for a present key and `Nothing` for a missing key. It takes exactly one key,
with no default argument. The receiver and key are observed, may be references,
and are evaluated once in source order. Lookup does not allocate: scalar values
are copied and immutable managed values are retained. The returned value survives
replacement of the entry or destruction of the dictionary. Constructing either
input expression still uses that expression's allocation policy.

`get` supports only non-affine values, like ordinary indexed reads: numbers,
booleans, strings, and enums whose payloads do not transfer ownership. Collections,
classes, and enums containing them are rejected, even for temporary dictionaries;
there is no hidden deep copy or alias to mutable storage. Use `pop` to remove and
take ownership of such values; element borrowing remains future work. The `Option` wrapper keeps
stored absence distinct from a missing key: a stored `Nothing` is returned as
`Some(Nothing)`.

`items.get(index)` provides the same optional read for lists, using an `i64`
index and returning `Option[T]`. Negative indices count from the end; empty lists
and out-of-range indices return `Nothing`, including extreme `i64` values. The
receiver and index are observed once in source order, may be references, and remain
usable. Lookup is constant-time and does not allocate or change the list. Managed
immutable results retain their existing storage, surviving entry replacement or
list destruction. As with dictionary `get`, affine elements are rejected: use
`pop` to remove and take ownership, or explicit copying with ordinary indexing.
There is no default argument. Unlike string `try_get`, list lookup does not create
new character storage and therefore needs no allocation-error result. Constructing
the receiver or index expression still follows its own allocation policy.

`dictionary.pop(key)` requires a mutable binding, mutable class field, or exclusive
reference and returns `Option[V]`. A hit removes the entry and transfers its value
into `Some`; a miss returns `Nothing` without changing the dictionary. All permitted
dictionary value types work, including lists, classes with custom cleanup, and
enums containing owned payloads. The removed value is never copied or destroyed by
removal; its returned owner is responsible for cleanup. Discarding that result
performs ordinary automatic cleanup. The removed key's stored owner is released.

`pop` takes exactly one key, with no default argument. Its key expression is
observed once before taking the receiver's exclusive loan, as with other collection
mutations. Key references are accepted, and an immutable key derived from the same
dictionary can be used. Active conflicting loans still prevent mutation. Temporary
dictionary receivers and set `pop` are not supported; sets use `discard`.

Removal itself does not allocate, and both buffers retain their capacity. Remaining
entries keep their insertion order; reinserting a removed key appends it at the end.
The initial implementation shifts entries and rebuilds buckets in existing storage:
successful removal scans the reserved table and rehashes the remaining keys,
while a missing key uses the normal hash lookup. Key evaluation and subsequent user cleanup retain
their own allocation policies. This is not a promise of constant-time removal.

`items.pop()` removes a list's last element; `items.pop(index)` selects an `i64`
index, including negative indices counted from the end. Both return `Option[T]`
and transfer the selected element's owner into `Some`. Empty lists and out-of-range
indices return `Nothing` without changing the list, including extreme `i64` inputs.
All supported list element types work, including nested collections and classes
with custom cleanup. The remaining elements preserve their order and the list
keeps its capacity; removal allocates nothing. Removing the last element is constant
time, while earlier removal shifts the remaining elements.

List `pop` requires a mutable binding, mutable field, or exclusive reference.
An explicit index (or reference to one) is evaluated once before borrowing the
receiver exclusively, so `items.pop(len(items) - 1)` is valid. A conflicting loan
that remains active afterward still prevents removal. Temporary receivers and
more than one index argument are rejected. Removed owners clean up normally,
including when the returned `Option` is discarded or a later `?` propagates.

`values.discard(value)` removes a set member and returns `True` if present,
otherwise `False`. A miss does not change the set. It requires a mutable binding,
mutable field, or exclusive reference and exactly one argument of the set's element
type (or a reference to it). The argument is observed once before taking the
receiver's exclusive loan and remains available afterward. Active conflicting
loans prevent removal; temporary receivers are not supported. User-defined class
methods named `discard` continue to use ordinary method dispatch.

Set removal releases the stored member's owner, reuses both buffers, and allocates
nothing. It shares dictionary hash-index rebuilding: a hit scans the reserved table
and rehashes remaining keys; a miss uses normal lookup. Reinsertion can reuse the
vacated capacity. Set iteration order remains unspecified. The Boolean result
distinguishes removal from absence, including for zero, `False`, and empty strings;
there is no exception or allocation-error result. Argument construction retains its
own allocation policy.

`list(iterable)` and `set(iterable)` convert supported iterables; `dict(d)`
transfers an existing dictionary value. Dictionary `keys()` and `values()` produce
new lists. `values()` on mutable payloads requires an owned temporary such as
`copy(d).values()` so a borrowed dictionary cannot expose mutable aliases.
Pair iterables and `items()` await tuples.

`dictionary.try_keys()` and `dictionary.try_values()` are fallible snapshot
operations returning `Result[list[K], AllocError]` and
`Result[list[V], AllocError]`. Both take no arguments, evaluate the receiver once,
and preserve insertion order. They allocate a new list, retaining existing
immutable element storage rather than deep-copying it. For non-affine values,
the receiver is observed and can be a binding, field, or reference; the snapshot
survives source updates and destruction. `try_keys()` also observes dictionaries
with affine values, without touching those values.

Like `values()`, `try_values()` with affine payloads requires an owned temporary,
whose values transfer to the result. A named binding or reference cannot expose
mutable aliases through this operation. Use `try_copy(d)?.try_values()` to
explicitly request fallible duplication while preserving `d`, or call it on a
function result to transfer its owned payloads, including custom-cleanup classes.
The temporary is consumed on both success and failure; failed allocation cleans
its original payloads normally. Borrowed receivers remain unchanged on failure.

The runtime reserves the entire entry buffer and list header before retaining or
transferring elements. Nonempty snapshots require two allocations; empty snapshots
require only the header. After reservation, copying the slots cannot allocate or
invoke user code. Capacity/layout overflow returns `AllocError.CapacityOverflow`;
allocation failure returns `AllocError.OutOfMemory`. The `Result` wrapper needs
no allocation. Receiver construction and user cleanup retain their own allocation
policies. Ordinary `keys()` and `values()` still terminate on allocation failure.

`items.reverse() -> ()` reverses a list in place through a named mutable owner,
class field, or exclusive reference. It takes no arguments and allocates nothing.
Element slots are reordered without cloning, retaining, or dropping payloads;
all permitted list element types are supported. Later list destruction follows
the new element order. Empty and one-element lists are unchanged. Temporary
receivers and shared references are rejected; the usual overlapping-loan rules
apply. This does not introduce a reversed iterator or reverse slice steps.

`collection.clear() -> ()` empties a list, dictionary, or set through a mutable
binding, class field, or exclusive reference. It takes no arguments. It retains
the collection's buffers and capacity, resets any hash index, and allocates no
runtime storage. Removed owners are dropped before returning: lists in element
order and dictionaries in insertion order with keys before values. Set order is
unspecified. User cleanup retains its own allocation/effect policy. Empty clear
is harmless, future insertions can reuse capacity, and independent immutable
owners survive. Temporaries, shared references, and conflicting loans are rejected.

`items.try_extend(other) -> Result[(), AllocError]` appends all elements of a
same-typed owned list. The destination requires a named mutable binding, class
field, or exclusive reference. The source is evaluated and moved before exclusive
access to the destination, as with `try_append`. It is consumed on both outcomes:
on success its elements transfer in order without cloning; on failure its owners
are dropped. Destination contents stay unchanged on failure. Preserve a source
explicitly with `items.try_extend(try_copy(source)?)`; references and arbitrary
iterables are not accepted as sources. Self-extension by moving the destination
is rejected by ownership checking.

Reserve the entire additional entry count before moving anything. When capacity
suffices, including empty sources, the operation allocates nothing; otherwise
reservation returns `OutOfMemory` or `CapacityOverflow`. After reservation only
slot transfer remains. All permitted list element types, including custom-cleanup
classes, work. Input construction and user cleanup retain their own policies.

`dictionary.try_update(other) -> Result[(), AllocError]` consumes a same-typed
dictionary, with the same receiver and source ownership rules as `try_extend`.
Existing keys retain their insertion positions and stored key owners, while
incoming values replace their payloads. New keys append in source insertion
order. Count keys absent from the destination and reserve all entry/hash storage
before any replacement, insertion, or payload destruction. A reservation failure
leaves destination contents and lookup behavior unchanged; the consumed source
is then cleaned normally. Capacity may change during preparation.

Once reservation succeeds, transfer source entries without copying. Replacement
drops old destination payloads in source traversal order, after installing each
new payload. Incoming duplicate-key owners are released; new-key owners transfer.
Replacing only existing keys, empty sources, and updates that fit reserved storage
allocate nothing in the runtime. User cleanup keeps its own effect/allocation
policy. Use `try_copy(source)?` to preserve an input; source references, pair
iterables, and keyword updates are deferred.

`set.try_update(other) -> Result[(), AllocError]` consumes a same-typed set and
adds its missing members. The destination requires exclusive access; the source
is consumed even on failure. Count absent members and reserve entries/buckets
before changing contents. Allocation failure leaves destination membership and
lookup behavior unchanged. On success, transfer new member owners and release
duplicate incoming owners while keeping the destination's existing owners.
Empty and duplicate-only sources need no allocation; sufficient reserved capacity
also avoids allocation. Set iteration order stays unspecified. Use `try_copy`
explicitly to preserve the source; general iterables, source references, and
multi-source update calls are deferred. Hashing/comparison of permitted member
types invokes no user code or allocation.

`items.try_slice(start, stop)` returns `Result[list[T], AllocError]` for a
forward list slice. Both arguments are required `i64` indices; start is inclusive
and stop exclusive. Negative bounds count from the end, bounds clamp to
`[0, len(items)]`, and a stop before start yields an empty list. Extreme signed
bounds cannot overflow. The receiver, start, and stop are observed once in that
order, including reference arguments. Slice syntax, omitted bounds, and steps
are deferred.

The result is a new list. Immutable elements are retained, with no deep copy;
its lifetime is independent of the source. Affine elements require an owned
temporary receiver and transfer only the selected owners. The temporary's other
elements are dropped in source order after the call; on failure all its elements
are dropped. Use `try_copy(items)?.try_slice(start, stop)` for explicit duplication
that preserves an affine source. Borrowed sources remain unchanged on failure.
The complete output buffer and header are reserved before any transfer (two
allocations for nonempty slices, one for empty slices). Capacity/layout overflow
and exhaustion return `AllocError`; input construction and user cleanup retain
their own policies.

`range(stop)`, `range(start, stop)`, and `range(start, stop, step)` use i64
arguments, exclude stop, and store only start/stop/step/length. A zero step or
length exceeding i64 is a runtime error. Negative steps and extreme i64 bounds
are checked using wider intermediate arithmetic. Range membership is constant
time. `%` uses the divisor's sign, like Python; division by zero is an error,
while `INT_MIN % -1` is zero.

`for name in iterable:` evaluates its iterable once and consumes owned collections
and generators. `for name in &collection` borrows instead, initially for copyable
elements only. Borrowed generator iteration is rejected; `next` accepts an
exclusive generator reference. An explicit `copy(collection)` provides a snapshot
when mutation of the original is needed during iteration. In each form,
lists yield elements, dictionaries keys, sets elements, ranges integers, and
strings Unicode scalar values as `str`. Loop variables and declarations are
block-local; iteration bindings are immutable and explicit `mut` declarations
remain permitted. Updates to enclosing mutable bindings persist. Even
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
a fixed native collection ABI with 128-bit slots; immutable metadata encodes the
already known element types. Metadata supports native storage/equality/printing, not
dynamic type inference. No per-element compilation or trait instantiation occurs.
Every program links the same precompiled Rust runtime archive; no runtime source
is compiled for individual programs.

The Rust runtime uses `Vec` storage and hash tables with ordered entries for
dictionaries and sets. Private builders append in place; literal and comprehension
construction is amortized linear under ordinary hash distribution. Public updates
also mutate in place; repeated `append` no longer copies existing contents.
Only explicit `copy` duplicates owned contents. Compiler-emitted type metadata caches
whether a type owns mutable contents, preventing copies from expanding shared
immutable enum graphs.
Heap payloads are reference counted. Standard sum wrappers live inline and
retain/release only their active payload. Replacing a local releases its previous
value; scope/function exits release remaining owners. Collection buffers are
reclaimed along with objects; type metadata lives in immutable program data.
Private expression temporaries
can remain until their enclosing scope exits. Retained helper operands are
implementation details, not permission to create source-visible mutable aliases.

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

Binary applications require exactly one module-level `main` function with no
parameters and a return type of `()` or `i32` (transparent aliases are accepted).
The native wrapper calls it once: `()` maps to status zero and `i32` is returned
as the process status. The operating system may truncate that status; small
nonnegative values are portable. `main` follows ordinary function return typing,
borrow checking, and deterministic cleanup, including early/nonzero returns.
It cannot be a generator. Other functions, including forward declarations, are
ordinary callable functions and have no startup effects just by being declared.

Module scope accepts `def`, `class`, `enum`, and `type` declarations, plus imports.
Executable statements and bindings belong inside functions. `main` locals are
not globals. Missing or invalid entrypoints are diagnosed by both checking and
compilation before running code or creating an output artifact. Compilation and
type errors execute nothing. Runtime errors can leave effects that have already
occurred. Imported modules require no entrypoint, and their `main` declarations
are ordinary functions. Library checking is available; library object/shared
library output remains future work.

## Modules and visibility

`import package.module`, `import package.module as alias`, and
`from package.module import Name as Alias` bind explicit names. Comma-separated
imports are supported. Imports exist only at module scope and do not execute
initializers. There are no globals, implicit transitive imports, relative paths,
wildcards, or re-exports. A module's imported names are private bindings.

One source root defaults to the entry file's canonical directory and can be set
with `--module-root DIR`. `a.b` resolves to `<root>/a/b.plenty`; directories are
namespaces, without `__init__` execution. File/directory name collisions are
errors. Canonical paths deduplicate imports and cannot escape the selected root.
Imported canonical filenames and directories must have identifier components
and a `.plenty` extension. Cycles report the actual file dependency chain. The
initial implementation caps graph depth at 128 and loaded modules at 4096.

`pub` exposes top-level functions, aliases, classes, and enums. Private names
are accessible only within their defining module; directory ancestry adds no
privileges. Class fields and methods require their own `pub`. Generated field
constructors are public only for public classes with all-public fields; an
explicit `__init__` has its own visibility. Public factories can construct
otherwise private constructors within their defining module. Automatic `__del__`
invocation is unaffected by privacy; direct lifecycle calls remain prohibited.

Public enums expose every variant. Resolved public signatures, public fields,
enum payloads, and public aliases cannot expose private nominal types, including
through transparent aliases and nested containers. Access checks cover field
reads, writes, borrows, method calls, and construction through aliases. Generated
structural printing and equality still include private fields; privacy is not
data secrecy. `pub` has no C ABI or binary-export meaning.

The compilation session loads each canonical file once, resolves explicit
bindings and lexical shadows, and assigns imported declarations qualified names
before type resolution. These names preserve nominal identity across import
aliases and diamond dependencies. Entry-module declarations retain their local
names; dependency declarations use canonical dotted module prefixes. All reachable
modules are checked and emitted into one object. Source paths accompany frontend
and independent IR/ownership diagnostics. Separate compilation and persistent
module caching are not implemented yet.

## Execution commands

- `plenty FILE` compiles into a private temporary directory, executes the native
  binary, then removes the directory. The child inherits standard input/output,
  the environment, and the current working directory. Its exit code is propagated;
  on Unix, signal termination is reported as 128 plus the signal number.
- `plenty --compile FILE -o OUT` produces a standalone executable.
- `plenty --check FILE` validates without native emission, linking, or execution.
- `plenty --check-module FILE` checks a library and its imports without requiring `main`.
- `--module-root DIR` selects the source root for modern file commands.
- `plenty` with no arguments displays help.

Both execution commands use the same compiler and embedded runtime. Running
and compiling require the system linker driver `cc`; checking does not. Neither
command invokes Cargo or rustc, and a relocated compiler binary needs no runtime
source files.
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
                                                                ↑
                                                 embedded Rust runtime archive
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
After the ownership/reference migration, the same debug workload and repetitions
measured 1.487 ms for parse/resolve/check and 53.140 ms for full AOT. These are
single-session diagnostic measurements, not a controlled performance comparison.
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

## Rust runtime packaging

`plenty-runtime` is a separate dependency-free workspace crate implemented in Rust.
Its internal C ABI exports use scalar arguments and pointers to fixed-layout
storage. Rust owns buffers; the compiler emits immutable type metadata. Narrowly scoped unsafe operations handle
generated-code pointers, flexible allocations, reference counts, and callbacks.
Plenty's static borrow checker still establishes source-level access permissions.

The compiler's build script invokes the selected rustc directly for Cargo's target,
avoiding recursive Cargo invocation and build-lock contention. It produces an
optimized static archive using ThinLTO and abort-on-panic, captures rustc's native
link requirements, and embeds both in the compiler. Per-program compilation
writes the Cranelift object and archive to a temporary directory, then runs `cc`
only as the linker driver. No C runtime sources remain. Rust compilation occurs
when building the compiler, with no rustc or LLVM invocation on the Plenty-program
compilation path. This is still native host compilation, not cross-compilation.

The public signatures and memory layouts are checked by native regression tests
and compile-time layout assertions. Standalone runtime tests also run under Miri
with exposed-provenance semantics for the ABI's packed pointer slots. The
`runtime-checks` compiler feature enables a counting Rust allocator for native
integration tests, covering buffers, raw object storage, and temporary allocations.
Allocation-free test regions count allocation attempts, including allocations
that have already been freed by the end of the region.
The relocated-compiler test rejects any runtime C/Rust compilation at link time.
See [plenty-runtime/README.md](plenty-runtime/README.md) for the boundary invariants
and validation commands.

A diagnostic measurement after this migration, on the same x86_64 Linux machine
and debug 100-function/6,194-byte workload with five measured repetitions, gave
1.499 ms for checking and 51.009 ms for complete AOT compilation with the optimized
archive. A first archive built without ThinLTO measured 100.508 ms for AOT, so
runtime optimization is deliberately paid during the compiler build. These are
single-session measurements, not controlled speedup claims against older baselines.

## One string type

There is one public `str`: an immutable sequence of Unicode scalar values.
Storage is valid UTF-8 with an explicit byte length and cached scalar count.
Embedded U+0000 is ordinary content; `\0` is a supported literal escape.
Neither literals nor dynamic strings have a trailing terminator. Equality and
hashing include every byte and do not normalize Unicode. `len` counts scalars,
not grapheme clusters; indexing (including negative indices) returns a one-scalar
`str`. Concatenation and indexing return independent values.

`text.startswith(prefix)` and `text.endswith(suffix)` return `bool` without
allocating. Each takes exactly one string (or reference), observes both operands
once in source order, and compares literal UTF-8 without case folding or
normalization. Empty prefixes/suffixes match every string, including empty input.
Optional bounds and tuples of alternatives are not supported. Input expression
construction retains its own allocation policy.

`text.find(needle)` and `text.rfind(needle)` return `Option[i64]` for the first
or last literal match. Positions count Unicode scalars, never UTF-8 bytes;
absence is `Nothing`, including when needle exceeds the source. An empty needle
matches at zero for `find` and at `len(text)` for `rfind`. The last match can
overlap an earlier match. Operands are observed once in source order, references
are accepted, and neither searching nor constructing the inline result allocates.
Converting the byte position to a scalar position scans the preceding prefix.
Optional bounds, regexes, case folding, and normalization are deferred.

`text.count(needle) -> i64` counts literal non-overlapping matches from left to
right without allocating. Empty needles match scalar boundaries, yielding
`len(text) + 1`, including one match in an empty string. No match yields zero.
Both strings are observed once in source order, including references. A source's
validated byte length leaves room for the header, so its boundary count fits i64.
There are no optional bounds, regexes, or normalization.

`text.try_strip()`, `text.try_lstrip()`, and `text.try_rstrip()` return
`Result[str, AllocError]`, removing Unicode White_Space from both ends, the
left end, or the right end respectively. The compiler's bundled Rust runtime
provides the Unicode classification; NUL and zero-width space are not whitespace.
Interior text is preserved byte-for-byte. No explicit character-set argument is
supported. The receiver is observed once, including references. Boundary scanning
does not allocate; creating the independent result requires one allocation even
for empty or unchanged output. Failures preserve the source, and the output
outlives it. No case folding or normalization occurs.

`text.try_repeat(count) -> Result[str, AllocError]` observes one `i64` count and
the receiver. Positive counts repeat the exact UTF-8 contents; zero and negative
counts produce an empty string, like Python repetition. Empty input produces
empty output for any count without iterating count times. Checked byte/scalar
multiplication and layout validation precede one final allocation, including
empty or single-copy results. Overflow returns `CapacityOverflow`, exhaustion
returns `OutOfMemory`, and the source remains unchanged. The runtime fills the
output by copying and doubling its initialized prefix without intermediate text.
The result owns independent storage. String multiplication syntax is deferred.

`text.try_slice(start, stop)` returns `Result[str, AllocError]`. It uses the list
slice's two required `i64` bounds, negative indexing, exclusive stop, and clamping,
but positions count Unicode scalars. Reversed bounds produce an empty string.
The receiver and bounds are observed once in source order, including references.
The result owns a new UTF-8 buffer, independent of the source; even empty and
full slices allocate one header/payload buffer. Allocation/layout failure is
recoverable and leaves the source unchanged. The runtime scans scalar boundaries
without an intermediate array, then copies the byte interval. This is linear in
the scanned text length; combining marks remain separate scalars and no Unicode
normalization occurs. Slice syntax and steps remain deferred.

`text.try_concat(other)` and `separator.try_join(parts)` return
`Result[str, AllocError]`. Both observe their inputs; `other` must be a `str`,
and `parts` must be a `list[str]` (or a reference to one). Empty list displays
receive that contextual type. Arbitrary iterables/generators are not accepted
yet. Receivers and arguments are evaluated once in source order, under the usual
borrowing rules. Building the argument itself retains its own allocation policy.

Joining inserts the separator between consecutive pieces, including empty
pieces. An empty list produces an empty string; a one-element list produces its
contents without a separator. The runtime first computes checked byte and scalar
lengths, then allocates one final header/payload buffer and copies the exact UTF-8
bytes. This currently includes empty and singleton results. There is no intermediate
text buffer or allocated array of pieces. Length/layout overflow returns
`CapacityOverflow`, allocator rejection returns `OutOfMemory`, and the inputs
remain unchanged. Error transport uses the allocation-free standard sum ABI.
Ordinary `+` shares the concatenation implementation but retains terminal failure
behavior. Ordinary string indexing, input, and formatting retain terminal
allocation failure behavior.

`text.try_replace(old, new)` returns `Result[str, AllocError]`. It requires two
`str` arguments (references are accepted) and observes receiver, old, and new
once in that order. Replace every literal, non-overlapping match from left to
right; inserted text is not searched again. An empty old string matches every
Unicode-scalar boundary, including both ends; replacing in an empty string with
an empty pattern inserts new once. Empty new strings remove matches. There is no
count limit, regex interpretation, grapheme matching, or normalization.

Count matches and check the final byte/scalar lengths and object layout before
allocating. Only the final output buffer is allocated, even for empty results,
unchanged results, or zero matches. Byte copying uses a second match scan, with
no intermediate strings, lists, or arrays of match positions. Overflow returns
`CapacityOverflow` and allocation failure returns `OutOfMemory`. All inputs remain
unchanged on either outcome, and successful output outlives them independently.
Input construction retains its own allocation policy.

`text.try_split(separator)` returns `Result[list[str], AllocError]`, observing
both strings (including references) without consuming them. The explicit separator
must be nonempty: an empty separator is an invalid-operation runtime error, like
an out-of-bounds index, and terminates without unwinding. `AllocError` reports
allocation failure, not invalid arguments. Whitespace splitting with an omitted
separator and a maximum-split argument are not implemented.

Matches are literal, non-overlapping, and scanned left to right. Leading, trailing,
and adjacent separators produce empty pieces; an empty input produces `[""]`.
No match produces a one-element list containing the input's contents. Multibyte
separators and embedded NUL bytes work without normalization. The runtime counts
pieces without allocating, reserves the result list, then allocates each piece's
UTF-8 storage independently. Even empty pieces currently allocate. A failed
allocation reclaims the list and every completed piece without changing either
input; successful pieces remain valid after the original strings are dropped.
The compiler supplies immutable result metadata; no descriptor allocation or
intermediate array of substrings is needed. Input expression construction and
ordinary indexing retain their existing failure policies.

`text.try_get(index)` returns `Result[Option[str], AllocError]`. The `i64` index
counts Unicode scalars, with negative indices relative to the end, just like
ordinary indexing. An out-of-range index (including either extreme `i64` value)
returns `Ok(Nothing)` without allocating. A valid index returns
`Ok(Some(character))`, using one checked allocation for the scalar's independent
UTF-8 string; allocator rejection returns `Err(AllocError.OutOfMemory)`. The
`Result` and `Option` wrappers themselves never allocate. The source is observed
and remains unchanged, and a successful character outlives it. Receiver and index
are evaluated once in source order; references to either input are accepted.
Lookup takes linear time to reach the scalar within UTF-8 storage; it does not
build a temporary character array. Ordinary `text[index]` shares this runtime
implementation but still traps on missing indices or allocation failure.

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
Compiler-emitted metadata links shared type nodes, so shared enum dependencies
do not expand exponentially. There is no runtime metadata parsing or allocation.

`Option[T]` and `Result[T, E]` are compiler-known concrete enum constructors,
without user generics or traits. Payloads may be integers, floats, bool, str,
collections, classes, other nonrecursive enums, or unit. References and generators
cannot be payloads. Enums can be list elements and dictionary values, but are
not dictionary keys or set elements in the initial closed hashable-type set.

`Some`, `Nothing`, `Ok`, and `Err` are compiler-known prelude names. Constructors
use the expected type from an annotation, argument, return, or enclosing typed
constructor/collection. `Some(value)` can infer its complete type from its payload;
`Ok`, `Err`, and `Nothing` require context because their missing type arguments
cannot be guessed. Inference is local and left-to-right, not a search across
later statements or other functions. Qualified forms and aliases remain supported.
Prelude names follow other built-ins: top-level function/type redefinitions are
rejected; local bindings can shadow expression names. Unqualified patterns always
denote the standard variants and get their type from the scrutinee. User-defined
variants still require qualification.

Unit payloads evaluate their argument for effects, then store a zero marker in
the ordinary runtime field slot. Matching may ignore or bind that payload; reading
a unit pattern binding yields the no-register unit expression, so it can be
returned from a unit-returning function. It does not introduce nullability.

`match` currently accepts enums. Each `case` names a variant and binds payload
positions to immutable locals or `_`; a whole-value `_` covers the remaining
variants. Coverage is exhaustive and checked before lowering. Duplicate variants,
redundant wildcards, incorrect payload arity, and wrong enum identities are
errors. Nested patterns, guards, OR patterns, and scalar matching syntax are
deferred. The scrutinee is evaluated once. Arm bindings are scoped locally and
may shadow outer bindings. Continuing arms agree on result type; arms ending in
return/break/continue do not contribute a join value. A function-tail match
produces its final arm expression, like the existing statement-form `if`.

Native user-defined enum values are immutable pointer-sized handles to tagged
records. A record contains a managed header, immutable type metadata pointer,
tag, and one 128-bit slot per active payload field. Equality compares nominal type, tag, and
payload contents, using IEEE comparisons for floats. Runtime metadata records
whether equality is reflexive; float-containing values cannot use pointer identity
as an equality shortcut because of NaN. Aggregate pairs are memoized during a
structural comparison to avoid expanding shared payload graphs exponentially.
Printing uses qualified variant names. No niche optimization,
stable external layout, or per-instantiation code generation is required.
Frontend coverage lowers to the existing scalar-tag match with an invalid-tag
trap fallback. The independent checker validates construction/projection types;
the structured frontend places projections behind the corresponding tag tests.

### Allocation-free standard sums and propagation

`Option` and `Result` use an inline 128-bit representation: one 64-bit terminal
payload and one 64-bit path of binary tags, outermost tag first. Nested standard
sums add tag bits without boxing; the existing 64-level type limit bounds the path.
The terminal payload is scalar bits or a handle to an independently owned heap
value. This representation preserves all integer and float bit patterns; it does
not reserve a null pointer or numeric sentinel as a source-level value.

Construction, passing, returning, matching, and `?` do not allocate a standard
sum wrapper. Copies of scalar-only sums and their equality comparisons are also
allocation-free. Payload operations retain their existing costs: creating a list,
concatenating strings, copying mutable contents, constructing a user-defined enum,
or formatting output can allocate. Cleanup may execute user code that allocates.
This is a wrapper guarantee, not yet a guarantee of recoverable allocation failure.

Native calls carry standard sums as integer pairs through Cranelift's internal
calling convention. Addressable locals, collection entries, record fields, and
generator slots use 16-byte storage, including scalar slots in this initial
uniform representation. This increases aggregate storage costs relative to the
previous 8-byte slots; compact per-type storage is a later optimization. The
runtime aggregate helper takes pointers to aligned input/output slots rather than
depending on a platform's C ABI for `u128`. Neither representation is a public FFI ABI.

Postfix `value?` evaluates its operand exactly once. `Ok(value)` and `Some(value)`
produce the payload. `Err(error)` returns `Err(error)` from the enclosing function;
`Nothing` returns `Nothing`. The enclosing function must return the same sum
family, and `Result` error types must be identical after alias resolution. Success
types can differ. There are no implicit error conversions or Result/Option
conversions. `?` on a unit success payload is a unit expression.

Propagation is an expression and can appear in calls, conditions, loops, and
comprehensions. It binds with other postfix operations, so `values?[0]` indexes
the unwrapped value. An early exit drops already-evaluated pending operands and
initialized locals, including hidden iterators/builders, in the normal cleanup
order. Later operands and statements do not run. The error payload transfers
ownership to the caller. Generators reject `?` because their declared return type
is `Generator[T]`, not a propagatable sum; explicit matching remains available.

The frontend emits a checked propagation operation with concrete source and
return types. The independent checker verifies family/error compatibility and the
enclosing signature. Native lowering branches before evaluating later operations,
returns the residual after cleanup, and continues with the success payload.

### Recoverable collection construction and growth

`AllocError` is a builtin nominal enum with two nullary variants:
`AllocError.OutOfMemory` and `AllocError.CapacityOverflow`. It shares the binary
inline representation of standard sums and never owns memory. Constructing,
copying, comparing, matching, or propagating it inside a `Result` does not
allocate. `AllocError` itself is not an operand or return family for `?`.
Unsupported allocator layouts remain a future concern when custom allocators exist.

Explicit collection types expose `try_new()` and `try_with_capacity(capacity: i64)`:
`list[T].try_new()`, `set[T].try_with_capacity(n)`, and
`dict[K, V].try_with_capacity(n)` return `Result` with the collection as its
success payload and `AllocError` as its error payload. Collection type aliases,
including imported aliases, expose the same constructors. This is concrete
builtin type-method syntax, not general generic functions or static class methods.

Both constructors produce an empty collection. `try_new()` allocates only its
owner header; `try_with_capacity(n)` also reserves room for at least `n` entries,
including hash storage where needed. The capacity expression is evaluated once.
Negative counts and impossible layouts return `CapacityOverflow` before attempting
any allocation. Allocator rejection at any subsequent stage returns `OutOfMemory`.
Construction cleans up any buffers already reserved before returning an error.
No partially initialized collection or user destructor is exposed.

The runtime validates and reserves buffers in a local Rust value before allocating
and initializing its owner header. Rust cleanup handles partial reservation; after
publication, the intrusive destruction queue releases buffers and frees the header
with its matching layout. Existing infallible collection construction uses the
same owner allocation/deallocation path with its existing terminal failure policy.

The following exclusive methods return `Result[(), AllocError]`:

| Receiver | Method | Contract |
| --- | --- | --- |
| `list[T]`, `set[T]`, `dict[K, V]` | `try_reserve(additional: i64)` | Reserve room for at least this many more entries beyond the current length |
| `list[T]` | `try_append(value: T)` | Append one element |
| `set[T]` | `try_add(value: T)` | Insert an element if absent |
| `dict[K, V]` | `try_insert(key: K, value: V)` | Insert or replace a value, preserving key insertion order |

They work on mutable bindings, exclusive references, and mutable class fields.
Callers handle the returned `Result` with `match` or propagate it with `?`.
Negative reservation counts and capacities exceeding the addressable buffer
layout return `CapacityOverflow`; allocator rejection returns `OutOfMemory`.
The return type remains fallible even when a particular call needs no growth.

Growth validates sizes and reserves both entries and hash storage before
committing a mutation. Failed reservation/insertion preserves logical contents,
order, and lookup behavior; callers must not depend on capacity being unchanged.
Hash rebuilding uses preallocated storage and never invokes user code. A
successful reserve permits that many subsequent insertions without storage
allocation. Existing-key replacement and duplicate set insertion do not grow
storage. Argument evaluation and user cleanup can still allocate independently.

Insertion consumes its arguments on success and failure. A failed insertion
destroys an owned input instead of returning it; a caller needing to keep the
input can reserve before moving it. The native helper borrows inputs and retains
only successfully stored values; the compiler releases its input owners on both
paths. The returned error needs neither formatting nor a heap-backed enum record.

`try_copy(value)` returns `Result[T, AllocError]`, where `T` is the observed
operand type. It uses the same borrowing and copyability rules as `copy`: an owned
binding is observed rather than consumed, reference operands copy the referred-to
value, mutable contents are recursively duplicated, and immutable storage such as
strings can be shared. Scalars and wholly immutable values require no allocation.
Classes with custom cleanup, aggregates containing them, and generators remain
uncopyable. The argument expression runs once; its own construction can still fail
under the failure policy of that expression.

A failed deep copy leaves the source unchanged and releases all newly owned
values. Collection copies reserve the final entry count before copying payloads;
temporary ownership guards cover pending keys, pending values, and partial
collections. Record copies track their initialized field prefix and release only
that prefix on error, then free the header directly. They never run a whole-record
destructor on a partial object. Active inline sum payloads follow the same rules;
inactive owned variants require no allocation. Ordinary `copy` shares this runtime
implementation but retains its existing terminal failure policy. Propagating a
failed `try_copy` uses the ordinary `?` cleanup rules without allocating an error.

This is an incremental API: existing `append`, `add`, indexed assignment,
literals, comprehensions, ordinary constructors, `copy`, string `+` and indexing,
generators, comparisons
that need memoization, and formatting retain their existing terminal failure
policy. The `try_` prefix makes the currently recoverable operations explicit;
it does not settle the eventual syntax for making *all* allocations fallible.
Whole-program OOM recovery is not yet promised. Custom destructors must avoid
allocating, or handle their own fallible operations, on an OOM recovery path.

Collection buffers remain owned by Rust `Vec` with the same fixed global allocator
for growth and deallocation. Runtime allocator switching is not exposed. Per-value
allocator handles, context lifetime, and matching deallocation must be implemented
before adding custom/global allocator selection; merely changing an allocation
function pointer would lose allocator provenance.

The instrumented runtime can reject every allocation after a chosen number of
successful allocator calls, including reallocations. Native tests cover every
allocation site in construction/growth/reservation and nested copying, retry, preserved contents, moved-input
cleanup, and allocation-free handling while allocation remains disabled.

## Classes: fixed-layout records

`class` declares a concrete record with typed fields and associated methods.
It provides familiar Python-shaped organization without inheritance, dynamic
attributes, class variables, properties, or runtime method lookup.

```python
class Point:
    x: i64
    y: i64

    def squared_length(self) -> i64:
        self.x * self.x + self.y * self.y

    def shift(self: &mut Point, amount: i64) -> ():
        self.x = self.x + amount
        self.y = self.y + amount

def main() -> ():
    mut point = Point(3, 4)
    point.shift(2)
```

Without `__init__`, the compiler generates a positional constructor taking all
fields in declaration order. An explicit `def __init__(self, ...) -> ()`
overrides it. Every field must be definitely initialized on every normal exit;
branches merge their initialization sets, and a loop alone cannot establish
initialization because it may run zero times. Already initialized fields may be
read. Passing or borrowing the whole partially initialized instance is rejected.
Initialization cannot be delegated to another method. Field defaults, keyword
arguments, static methods, and constructor overloading are deferred.

A method's first parameter is `self`. Bare `self` means `self: &Class` in
ordinary methods and `self: &mut Class` in `__init__` and `__del__`. Other
parameters and every return require explicit types. Mutating ordinary methods
declare `self: &mut Class`; calling them requires a mutable owner or exclusive
reference. Calls borrow the receiver and reborrow reference arguments for the
duration of the call. Temporary owned receivers are supported without permitting
escaping references.

Every class instance moves on assignment and owned argument passing, including
records containing only integers. `copy(instance)` recursively duplicates owned
fields, but is rejected if the class or any nested value has custom destruction.
Field reads copy immutable values; owned fields must be observed, explicitly
copied, or borrowed. Partial moves out of classes are deferred. Structural
equality compares the nominal type and field values; printing produces
`Point(x=3, y=4)`. These operations do not invoke user-defined magic methods.

`&point.x` and `&mut point.x` borrow stable field slots, including nested
class fields. Loans conservatively cover the whole root binding: borrowing
`point.x` also prevents conflicting access to `point.y`. Collection-valued
fields support in-place updates such as `record.items.append(value)`.
Whole-instance replacement through `*reference = instance` is rejected for
classes, including class-valued field references; assign an owning binding or
a named field instead. This also prevents a destructor from replacing its dying
receiver through an alias.

Fields may contain classes, enums, and collections, but not references,
generators, or unit. Acyclic forward declarations and aliases are supported;
recursive layouts remain rejected. Methods become statically resolved native
functions. Class instances currently use one owned heap allocation with a runtime
header, concrete type metadata, an optional destructor adapter, and 128-bit field
slots. This representation favors simple lowering and fast compilation; it is not
a public FFI layout guarantee. The compiler caches nesting, copyability, and
destructor flags on nominal metadata.

## Ownership and reclamation

Classes, mutable collections, and aggregates containing them move on assignment and owned
argument passing; independent duplication requires `copy(value)` or a successful
`try_copy(value)`. Immutable `str`
and enums containing only immutable values may share storage. Copyability is
cached on concrete enum and class metadata so shared type graphs are not traversed repeatedly.

Every managed expression operand, live local, stored field, and frame capture
has one owner. Loading a copyable value retains its immutable storage. Moving
an owned value clears the source ownership slot. Stores evaluate the RHS, release the
old owner, then transfer the new one. Scope exits, loop exits, and function exits
release locals; compiler-private temporaries are bounded by local slots.
Tail-call arguments are owned before caller cleanup. Observable resource cleanup
prevents tail-call rewriting in functions with resource-bearing slots. Traps terminate the process without unwinding language scopes.

Runtime objects share `{u64 refs, destroy_callback}`. Heap objects start with one
reference; literal strings use an immortal count. Helpers borrow arguments and
return owned managed results, including retained projections and builder aliases.
Buffers have explicit owners too; type metadata is immutable program data. Destruction uses an
iterative queue, avoiding recursive C-stack growth through owned value graphs.
Reference counts are non-atomic; the language has no concurrency. The current
unique mutable ownership and restricted aggregate types prevent source-visible
ownership cycles.

Collections, classes, generators, and enums with owned payloads are affine. The independent
ownership pass tracks
definite availability of local slots through structured branches. Only continuing
arms join. A move on one branch makes the binding unavailable at a later join
unless reinitialized; exiting branches are excluded. Each loop backedge,
including `continue`, must preserve availability of outer owners available at
entry. A move followed by mutable reinitialization is accepted; a move reaching
a backedge is conservatively rejected. Break paths join the zero-iteration path.
A generator cannot be copied or stored in an aggregate. Collection/enum payloads
can be owned mutable values: construction transfers ownership, and consuming
matches transfer their bound payloads. Enums with such payloads are also affine.

## Deterministic destruction

Plenty uses ownership-driven destruction, without a tracing garbage collector.
The compiler inserts cleanup on ordinary control-flow exits. This applies to memory
and custom class cleanup. Files and foreign handles remain future library work.
Reference-counted immutable string storage is compatible with this model: releasing
a string owner decrements its count and frees dynamic storage at zero. Borrows do
not acquire ownership or independently destroy their referents.

The runtime reclaims built-in values, classes, and abandoned generator frames;
`drop(value)` optionally consumes an owner early. A class may implement
`def __del__(self) -> ()` for custom cleanup; automatic field cleanup follows it.

- A live owned local is dropped when its lexical scope exits, in reverse binding
  declaration order. Inner scopes clean up before outer scopes. This includes
  `return`, `break`, and `continue` for the scopes each exit crosses.
- Moving a value transfers its cleanup obligation; the moved-from place is not
  dropped. At a branch join, cleanup is conditional on whether the place still
  holds a live value. Replacement evaluates the RHS first, destroys the previous
  initialized value, then installs the new one.
- The `drop(value) -> ()` builtin consumes an owned value and performs early
  destruction. It cannot destroy a borrowed referent or consume an owner while a
  conflicting loan is live. For a shared immutable string it releases that owner's
  share; it cannot force other owners' storage to be freed.
- Borrow lifetimes may end at last use. Observable destruction of owned resources
  remains at the specified scope exit or explicit `drop`, not an optimizer-chosen
  last use. Borrow checking treats destruction as an exclusive access to the owner
  and everything its cleanup may access.
- Owned expression temporaries clean up at the end of their full expression unless
  transferred to another owner. A block's result is transferred before its other
  locals are dropped. Initially reject escaping borrows of temporaries rather than
  adding implicit temporary-lifetime extension rules.
  Loop conditions and each comprehension iteration finish their own temporaries.

`__del__` takes exclusive access to the value and returns `()`. A general trait
system is not required. The hook runs before automatic field cleanup. Class fields and active
enum payloads drop in declaration order; list elements drop in index order. Dictionary
entries drop in insertion order, with each key before its value. A child finishes
destruction before the next sibling begins. Types with custom destruction cannot be
copied, even explicitly, and cannot be partially moved or have their destructor
called directly as an ordinary method. The hook cannot let references to the dying
value escape. Nested owned fields are cleaned automatically; hooks manage only the
additional resource-specific work.

Drop hooks have no recoverable return value and cannot yield. Resources that need to
report shutdown errors should also offer an explicit operation returning `Result`;
their destructor provides fallback cleanup. An internal state records an already
closed resource so explicit close followed by drop does not release it twice.
Normal `Result` error paths still run scope cleanup. Fatal traps and process aborts
do not unwind, so cleanup is not promised in those cases.

A suspended generator retains its live owned locals until resumption, completion,
or destruction. Destroying its frame cleans those values without resuming the body;
code following a `yield` is not a cleanup hook. Observable cleanup must follow the
same scope and field rules, including captures in a never-started frame.

Drop order also constrains tail-call optimization. A normal call in tail position
must retain caller-owned resources through the call when their specified destruction
occurs afterward. Do not move an observable destructor before a call merely to emit
a native tail call. Initially disable that optimization when such cleanup remains,
or when a callee borrows caller-local storage. The current conservative check
uses parameter/local types, so it also disables tail calls after explicit early
drops in a function with resource-bearing slots. More precise cleanup analysis
could recover those tail calls later. Generators count as resource-bearing because
their frames may capture classes regardless of their yield type.

The destruction queue processes nested values in depth-first declaration order
without recursive native stack growth through automatic field cleanup. A hook
runs through a native ABI adapter before its fields are queued. Drops performed
inside the hook drain synchronously, preserving their order relative to its other
effects; temporarily suspending the outer queue prevents sibling cleanup from
running early. Source-level recursive hook calls can still use the native stack.
Exact cleanup traces, allocation accounting, and sanitizer tests cover this path.

Reference: [Rust destructor scopes and field cleanup](https://doc.rust-lang.org/reference/destructors.html).
These are Plenty's selected rules; they do not require copying every Rust feature.

## Public borrowing — bindings and class fields

[References and explicit copying](docs/proposals/references-and-copying.md) records
the accepted ownership decision. References to named bindings and borrowed function
parameters and their class fields are implemented as `&T` and `&mut T`. `str` remains the sole string
value type; `&mut str` permits replacement of a string binding, not byte mutation.

Reference bindings are immutable and initialized by a direct `&name` or
`&mut name`, including field paths; reassignment and implicit reference aliases are rejected initially.
Reborrowing an existing reference is supported. An exclusive parent may lend shared
or exclusive access, with conflicting parent access prohibited while the child is
live. Calls automatically reborrow reference arguments, using declared signatures
only. Shared references cannot be upgraded to exclusive ones.

The frontend records loan origins and reads/writes of whole binding places.
The independent checker computes backward loan liveness over explicit control-flow
edges to a fixed point, including backedges and early exits. A conflicting write,
move, drop, or exclusive borrow is rejected while a loan remains live. Shared
reads conflict with live exclusive loans. Parent origins are tracked through
reborrows; access through a parent conflicts with a live child. Copies of immutable
values finish their read immediately; observations of mutable collections hold
temporary shared loans through the operation that consumes them.

Native references address 128-bit local storage slots, generator frame slots, or
fixed class field slots.
Functions taking addresses spill their locals; ordinary functions retain SSA locals.
Borrowed parameters already carry an address. Internal retained operands protect
temporary storage lifetime, but the static checker establishes access permissions.
Reference calls retain the caller frame, so native tail calls do not invalidate it.

Collection element references, partial moves, stored references, and returned
references remain rejected. Class field loans conservatively overlap at the root. A generator cannot capture reference parameters or retain a live
loan across `yield`; short borrows completed within one resume are permitted.
No lifetime annotation syntax or general trait system is required for this subset.

Prefer last-use/flow-sensitive loan checking over lexical-lifetime rules.
Polonius is the relevant Rust work: it models relationships between reference
origins and loans over control flow. Rust's Polonius alpha was enabled on
nightly in August 2026; stabilization and formal modeling remain work items.
The old standalone Datalog engine is not automatically the current rustc
implementation. We should reuse concepts and test cases, not assume that
adding a crate supplies a sound checker for Plenty.

The first implementation supports local borrows without returned/stored references.
Once projected source places,
aliasing, moves, reborrows, joins, and drop points are modeled, add a restricted
reference-return rule whose origin is unambiguous from the signature. Reject
ambiguous cases before introducing lifetime syntax. Never infer cross-function
borrowing contracts by inspecting callee bodies. A small sound subset is
preferable to a permissive checker with gaps.

Correctness gates include use-after-move, conflicting shared/exclusive loans,
mutation during a live shared borrow, branch-dependent loans, reborrows,
returning local references, partial moves, and ownership across control-flow
joins. Extend regression coverage and cleanup checks whenever the supported
reference subset grows; the current checker is not a complete Polonius implementation.

Sources informing this design:

- [Polonius alpha nightly announcement](https://blog.rust-lang.org/2026/08/04/enabling-polonius-alpha-on-nightly/)
- [2026 Polonius stabilization/modeling goal](https://goals.rust-lang.org/2026/polonius.html)
- [Borrow checker roadmap](https://goals.rust-lang.org/2026/roadmap-borrow-checker-within.html)

## Native generators

A function containing `yield` declares `Generator[T]`. Calls evaluate arguments
and create an owned frame without running the body. Each resume executes native
code until a statement-only `yield value`, bare `return`, or fallthrough.
Yield types are exact; owned payloads transfer into the yielded value.
Nested generator yield types, reference yields, and unit are
rejected. Generator functions cannot return a value. An ordinary factory without
`yield` may return another generator by moving it.

`next(g)` requires a named mutable generator binding or an exclusive reference
to a generator, and returns `Option[T]`.
This compiler-known operation borrows the owner only for the call. Exhaustion
is stable. `for`, comprehensions, and iterable collection constructors consume
generators; `break` destroys their hidden iterator owner without executing later
generator statements. Generator assignment, arguments, and returns transfer
ownership. Printing, equality, length, and membership are not defined on them.
There is no yield-from, send/throw, generator expression, public reference across
suspension, or async/await.

Each generator has a concrete constructor and native resume function. The frame
owns parameters and all locals in fixed 128-bit slots, plus immutable slot-type metadata,
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
Iteration wraps each resume result in an allocation-free inline `Option`.
Optimizing frame liveness remains a later runtime improvement.

`set.try_union(other) -> Result[set[T], AllocError]` observes both same-typed sets
and returns their unique members in an independent set. Inputs can be references
or the same set. All output storage is reserved before retaining member owners;
allocation failure leaves both inputs unchanged. Immutable strings may share
storage. Nonempty results use three allocations (buckets, entries, owner header),
empty results only the header. Iteration order is unspecified.

`set.try_intersection(other)` has the same result and borrowing contract as
`try_union`, selecting only common members. It reserves for the actual result
size, so disjoint inputs need only an empty output owner header.

`set.try_difference(other)` selects receiver members absent from `other`, using
the same fallible, borrowed, exact-capacity contract. Unlike union and intersection,
the operands are not interchangeable; subtracting a set from itself yields empty.

`set.try_symmetric_difference(other)` selects members present in exactly one
input. It shares the same fallible construction contract; equal inputs produce
an empty result. Set algebra methods accept exactly one same-typed set, not
arbitrary iterables, and do not imply overloaded arithmetic operators.

Set relationship methods take one same-typed set, including shared references,
and return `bool`. They observe both operands once, left to right, without moving
them or allocating. Empty sets are subsets of all sets and disjoint from all sets.
No iterable conversion, comparison operators, or heterogeneous key coercion is implied.

## Next milestones

For a useful basic feature set, prioritize these capabilities. This is a proposed
sequence; API syntax and the reference contracts still require design work.

Floating-point types, unit payloads, and the standard sum-type prelude are now
implemented. The new design review changes the recommended priority:

1. Explicit binary `main`, absolute imports, module-private declarations with
   `pub`, qualified dependency identities, and multi-file tutorial examples are
   implemented. Package management and separately cached module objects remain
   future work; neither is required for the next language features.
2. `?` for `Result` and `Option`, allocation-free standard sum wrappers, and
   immutable runtime metadata are implemented. Error payload construction still
   matters: scalar error codes need no allocation, while user-defined enum
   records currently do. Recoverable OOM needs allocation-free error payloads
   as well as fallible runtime operations; wrapper layout alone does not promise it.
3. Complete fallible allocation and allocator provenance (collection construction,
   reservation, insertion, explicit copying, and text concatenation/joining/splitting
   and checked character lookup, slices, replacement, and dictionary snapshots now have recoverable
   `try_` APIs), then expand text,
   collection, input/file, and argument APIs under those rules. Add allocation
   failure injection and checks for valid state/cleanup on every failure path.
4. Broaden borrowing for elements, owned-element iteration, disjoint class fields,
   and restricted returned references. Add concrete context managers using the
   same cleanup machinery; stored references remain a later extension.
5. Add explicit generic functions and structural protocol constraints, with direct
   inherent-method lookup and measured instantiation caching. No import-sensitive
   method activation, specialization search, or implicit dynamic interface values.
6. Introduce C ABI adapters and trusted interface declarations, then library output
   and typed runtime loading. Reserve these boundaries during steps 1–3; do not
   expose current internal object layouts as a public foreign ABI.

Move the tutorial to independently runnable literate sources and generated
Markdown alongside the entrypoint migration if convenient, retaining reviewed
expected results. Tuples/unpacking, recursive types, and richer standard-library
APIs remain useful follow-on work. Threads and explicit parallel operations need
thread-transfer rules and a compatible runtime before automatic parallelization
is considered. SIMD needs a target/portable-lowering design of its own. Async/await
remains out of scope. See the linked proposals for scope, tradeoffs, and open
decisions; this ordering does not mean the future features are already approved
down to their syntax.

Improve diagnostics and measure compilation latency throughout these steps,
including archive extraction and native linking. Extend ownership regression,
allocation, and sanitizer checks as reference support grows. Evaluate additional
Polonius-style precision against compilation cost rather than treating a complete
Rust-like borrow checker as a prerequisite for a usable first version.

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
archive extraction and linking are included; no latency claim without measurement.
