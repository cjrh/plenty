# Generics, structural protocols, context managers, and error propagation

Status: design proposal, 2026-10-05. Nothing in this document changes the
implemented language. Syntax below is illustrative. `DESIGN.md` remains the
implementation status reference.

## Starting point

Plenty already has concrete classes and inherent methods, `Option[T]` and
`Result[T, E]`, checked local/field borrowing, and deterministic cleanup.
`__del__` cannot fail recoverably. Cleanup already runs on `return`, `break`, and
`continue`. There are no user-defined generics, protocols, returned references,
stored references, context managers, or `?` yet. Built-in parameterized types
are compiler-known special cases, not evidence of a general generic system.

The recommendations are:

- Add `?` directly for `Result` and `Option`; it needs no trait system.
- Use explicit generic parameters with structural protocol constraints.
- Resolve protocol methods to the concrete type's inherent methods. Imports
  identify names; they never activate extension methods or implementations.
- Implement concrete context managers independently of generics. A returned
  reference from `__enter__` requires additional borrowing rules, however.
- Begin with infallible scope exit and explicit fallible resource operations.
  Defer automatic fallible exit until error composition has a complete design.

## Structural protocols with explicit generic parameters

Python protocols permit structural conformance without explicit inheritance.
Go interfaces likewise describe sets of implementing types through methods.
Both provide useful precedents; Plenty can adopt this idea without inheriting
Python's dynamic model or Go's runtime interface values. [Python typing
specification](https://typing.python.org/en/latest/spec/protocol.html),
[Go interface specification](https://go.dev/ref/spec#Interface_types).

Proposed declarations:

```plenty
protocol Writer:
    def write(self: &mut Self, data: &str) -> Result[(), WriteError]

def write_heading[W: Writer](writer: &mut W, heading: &str) -> Result[(), WriteError]:
    writer.write(heading)?
    return Ok(())
```

`Self` means the concrete implementing type and is a compiler-defined name
inside protocols. `WriteError` must be declared or explicitly imported.
`protocol Writer:` follows Plenty's existing declaration shape. Protocol method
declarations have signatures and no bodies; punctuation remains a parser decision.

A concrete class with an accessible matching `write` method satisfies `Writer`.
No registration, inheritance, global implementation search, or implementation
import is needed. A caller need not import `Writer` merely to call
`write_heading`; the generic function's signature already names its constraint.
Calling a concrete object's `write` method never depends on whether the caller
imported this protocol. Across module boundaries, required concrete methods must
be `pub`. Protocol conformance must not expose private methods through a public
generic function.

Begin with required methods only. Defer protocol fields, default methods,
inheritance/composition syntax, associated types, generic protocol methods,
operator overloading, and recursive protocol constraints. A method-only protocol
avoids exposing field representation and mutable-field variance questions. Python
supports substantially more, including mutable attributes and generic protocols;
its variance rules illustrate the additional complexity. [Python generic
protocols](https://typing.python.org/en/latest/spec/protocol.html#generic-protocols).

### Type checking and ownership

The declaration's constraints determine every operation permitted in a generic
body. Do not accept arbitrary duck typing and discover missing methods only by
expanding the first call site. Diagnose a missing constraint at the generic
definition; diagnose a failed concrete conformance at the instantiation.

Initially require exact method parameter and return types after substituting
`Self` and explicit type arguments. Include all ownership-relevant properties:

- `&Self`, `&mut Self`, and an eventual owned receiver are distinct contracts.
- `T`, `&T`, and `&mut T` parameters are distinct. Matching names alone is
  insufficient, and mutable access cannot substitute for shared access.
- Returned references, when supported, include their source/escape contract.
- Fallibility is represented by the exact `Result` return type.
- Future thread-transfer and thread-sharing guarantees are separate checked
  capabilities; having appropriately named methods does not establish them.

An unknown `T` must be treated conservatively as an owned value. It can move,
borrow, and drop; it cannot be silently duplicated because one instantiation
happens to be an integer. An unconstrained generic identity function is useful:

```plenty
def identity[T](value: T) -> T:
    return value
```

To duplicate a generic value, require an explicit supported copy capability or
a user-provided operation. Allocation failure makes the exact copy API part of
the fallible-allocation design. Do not decide it accidentally in the generic
checker. In particular, a structural `copy` method is an explicit method call;
it does not silently grant the compiler permission to copy values.

Invariant generic types and exact signature matching are a deliberately small
first subset. More permissive variance or receiver adaptation can be evaluated
later against actual examples. Protocol satisfaction checks shape and types,
not behavioral laws: a method called `write` might still have incorrect behavior.
Use nominal wrapper types when a domain requires explicit opt-in or distinction.

### Compilation and representation

Type-check generic bodies against symbolic constraints, then instantiate only
reachable concrete uses. Lower those instances to ordinary typed Plenty IR and
Cranelift functions with direct calls. Cache by declaration identity plus concrete
type arguments; keep the cache independent of import spelling. Report both the
generic definition and instantiation chain in diagnostics.

Monomorphization is simple at runtime but can increase compilation time and
binary size. Measure instance counts and time, reuse repeated instances, and
diagnose recursively expanding instantiations. A documented implementation limit
is preferable to compiler hangs. Ordinary recursion at the same type arguments
does not need a new instance. Keep type-level evaluation, specialization, and
overload search out of the first implementation.

Initially `Writer` is a constraint, not a storable runtime type. Write
`def f[T: Writer](x: &mut T)` explicitly. Defer `def f(x: &mut Writer)` rather
than letting it ambiguously mean either an implicit generic or dynamic dispatch.
Likewise, a heterogeneous `list[Writer]` is not available in this first model.
Generic functions can precede generic classes/enums; the latter require explicit
layout, drop, and recursive-type rules.

An erased interface value could later use explicit syntax such as `dyn Writer`,
with a data pointer and method table. It needs its own ownership/lifetime rules,
representation, and restrictions on methods using `Self` and generics. It is
not necessary to introduce those constraints for static generic calls. Rust's
dyn-compatibility rules are a useful checklist for this later feature, not a
requirement to copy its surface design. [Rust dyn
compatibility](https://doc.rust-lang.org/reference/items/traits.html#dyn-compatibility).

## `?` is a small language operation

For `Result[T, E]`, `value?` evaluates `value` once, yields its `Ok` payload, or
returns `Err(e)` from the enclosing function. That function must return
`Result[U, E]` with the same error type after alias resolution. For `Option[T]`,
it yields the `Some` payload or returns `Nothing` from a function returning
`Option[U]`. A unit success remains useful: `write(data)?` is an expression
statement yielding `()`.

Use postfix precedence so `open(path)?.read()?` has a predictable parse. Preserve
left-to-right evaluation and full-expression temporary cleanup. Move owned
payloads once; do not copy the enclosing sum or its payload. On propagation,
clean up exactly the scopes that an explicit `return` would cross, including
partially evaluated expression temporaries and later context-manager exits.

Propagation must not allocate merely to rebuild `Err` or `Nothing`. This is an
architectural requirement for recoverable allocation failure, not a property of
today's heap-backed enum representation. The implementation needs an
allocation-free error carrier or an equivalent verified lowering; reusing an
existing container is insufficient if its type metadata or ownership contract
changes. Error mapping performed explicitly by user code may itself be fallible,
but the primitive propagation path and OOM error construction must work without
allocating. See [memory, parallelism, and SIMD](memory-parallelism-and-simd.md).

Rust supplies a familiar precedent but permits conversion on `Result` error
propagation. Plenty should initially require exact error types and explicit
conversion using `match` or a later named mapping operation. Do not implicitly
convert `Option` into `Result`, infer a new union error type, or add an extensible
`Try` protocol in this step. [Rust try-propagation
expression](https://doc.rust-lang.org/reference/expressions/operator-expr.html#the-try-propagation-expression).

Reject `?` in `__del__`, in unit-returning `__init__`, and in generators until
generator completion/error semantics are designed. Fatal traps remain aborts;
`?` is ordinary typed control flow, not exception unwinding. A function entrypoint
returning `Result` would require a separate startup/status-reporting convention.

## Context managers without a trait dependency

Python's `with` calls `__enter__`, binds its result, and arranges `__exit__` on
leaving the body. Python also supplies exception information and supports
suppression. Plenty has no Python exception model to reproduce. [Python context
manager protocol](https://docs.python.org/3/reference/datamodel.html#context-managers),
[Python with statement](https://docs.python.org/3/reference/compound_stmts.html#the-with-statement).

The compiler can recognize concrete class methods directly, just as it already
recognizes `__del__`. There is no generics prerequisite. A later structural
protocol can describe this behavior for generic functions without changing
concrete lookup.

Recommended first contract:

```plenty
# Proposed signatures on a concrete class, not implemented today:
def __enter__(self: &mut File) -> &mut File
def __exit__(self: &mut File) -> ()

# Acquisition reports failure before the context has been entered:
with open_file(path)? as file:
    file.write(data)?
    file.flush()?
```

`open_file`, `File`, and its methods are illustrative APIs, not present library
features. In this shape, fallible acquisition happens before `with`, entry is
infallible, and exit performs infallible resource release. Durable writes and
other operations whose failure matters remain explicit `Result` operations.
Do not pretend that dropping a buffered file proves its data was persisted.

Keep Python's meaning of `as`: it binds the result of `__enter__`, not the manager
by an unrelated special alias rule. Owned entry results and unit entry are also
possible. The common `&mut self` result is the important missing capability:
the compiler must prove its origin is the hidden manager and prevent it escaping
the scope. It must keep that manager alive, prevent moves/replacement while the
loan exists, make the hidden owner inaccessible while the exclusive borrow is
active, and end the loan before calling `__exit__`. This should reuse a
general, narrow returned-reference rule rather than invent an unchecked exception
for `with`. A smaller first slice can support entry methods returning owned
values or `()` without returned-reference support.

Evaluate the manager expression once and move an owned result into a hidden
mutable local. Supporting `with &mut existing` can follow after specifying
reborrowing; do not implicitly consume and secretly restore an existing owner.
For every successfully entered context:

1. Run the body. Evaluate and preserve any pending return value before cleanup.
2. Destroy body-local owned values in normal reverse binding order, and finish
   borrows of the manager. Any owned `as` binding is body-scoped too.
3. Call `__exit__` exactly once, then destroy the hidden manager normally.
4. Continue the pending fallthrough, `return`, `?`, `break`, or `continue`.

Nested contexts exit in reverse order. Failed acquisition does not enter the
context. If entry later becomes fallible, failed entry must not call `__exit__`;
normal manager destruction still cleans partially acquired state. `__del__`
provides fallback release and must recognize an already closed manager so that
exit plus destruction does not release its resource twice. Fatal process aborts
are outside this guarantee, consistently with ordinary destruction.

The compiler's exit bookkeeping and release path must not require allocation.
An infallible exit signature alone cannot prove arbitrary user cleanup avoids
allocating: the eventual allocation-failure rules must also constrain cleanup
implementations and ensure they can release resources under OOM. In particular,
collecting exit errors into an allocating list would defeat recoverability.

Reject a returned/yielded borrow of the hidden manager. Initially reject `yield`
inside `with`: retaining manager state and an exit obligation across suspension,
and exiting abandoned generators, need explicit frame support. A later version
can support suspension with owned managers once its loan and cleanup rules are
verified. No async context managers are planned.

### Fallible exit is a separate feature

If a context's exit fails while its body is already returning an error, there
are two errors. Silently overwriting either is a poor default; changing `break`
or a successful `return` into a failure also needs an explicit enclosing type.
Python exception suppression does not solve this typed-control-flow problem.

Start with `__exit__ -> ()` and fallible explicit operations such as `flush`,
`commit`, or `finish` inside the block. That gives an honest small feature now.
For a later transactional context, a scoped expression could return an explicitly
composed error enum with `Body(E)`, `Exit(F)`, and `Both(E, F)` cases. Its shape,
allocation behavior, pending return values, nested exits, and handling of loop
control must be specified before introducing fallible `__exit__`. Composing
errors must itself work under allocation failure. A context must not implicitly
swallow or transform a caller's error without visible syntax.

An exit-reason enum distinguishing normal completion, return, and error may be
useful later. For the first contract, transactional success is explicit through
`commit()?`, and exit rolls back uncommitted state. This avoids assuming that
leaving a block normally is sufficient evidence for a successful transaction.

## Milestones and verification

1. Add `?` for the existing sums. Test nested expressions, unit success, owned
   payload movement, incompatible errors, and exactly-once cleanup on propagation.
2. Settle module visibility and the representation of generic declarations.
   Add unconstrained generic functions, then required-method structural protocols
   with exact signatures. Measure compile time and emitted instance counts.
3. Add the narrow returned-reference contract and its negative borrow tests.
   Add concrete `with` using the same exit machinery as existing control flow.
   Test nested contexts, acquisition failure, return/break/continue/`?`, escaping
   borrows, and repeated-close prevention.
4. Add generic classes/enums and parameterized protocols when practical library
   examples require them. Keep erased interfaces and fallible context exits as
   explicit later decisions.

Open decisions include protocol signature punctuation, an explicit conformance
assertion for library authors, exact returned-reference syntax, whether initial
contexts allow owned entry results to escape, and the eventual fallible-exit
syntax. None requires an import-sensitive trait lookup system.
