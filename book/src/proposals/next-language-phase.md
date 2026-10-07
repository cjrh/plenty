# Plenty's next language phase

> Active tasks and priorities live in the [backlog](../backlog.md). Staging below
> records design dependencies, not scheduled work; some syntax and assumptions
> are historical. See the [implementation status](../design/04-implementation-status.md)
> and [reference](../design/index.md) for current behavior.

This is a design review, not an implementation announcement. It consolidates the
entrypoint/module investigation and three concurrent research proposals. The
implementation baseline is `b690afe`; the [reference](../design/index.md) remains the
reference for what programs can use today. Syntax and sequencing below are
recommendations for review.

Several directions in this review have since been implemented or revised. Check
the [implementation status](../design/04-implementation-status.md) rather than
treating the sketches below as either current syntax or outstanding tasks.

The proposed direction fits Plenty: explicit names, structural interfaces, native
ownership and cleanup, and predictable ahead-of-time compilation. The biggest
changes to make early are module identities, an allocation-failure representation
that does not allocate, and a clear boundary between Plenty's private runtime
representation and the C ABI. Context managers need no general trait system.
Threads will need more runtime work than their surface syntax suggests.

| Topic | Recommendation | Main dependency or limitation |
| --- | --- | --- |
| Binary entrypoint | Require `def main() -> ()` or `-> i32` in the entry module | Replace current top-level execution and migrate examples |
| Modules and visibility | Absolute imports; private declarations by default; explicit `pub` | Stable declaration identities and path-aware diagnostics |
| C interoperation | Reusable trusted interface files, checked adapters, ordinary C ABI | Explicit ownership, lifetime, layout, and error contracts |
| Context managers | Concrete `__enter__`/`__exit__` methods and deterministic `with` cleanup | Returned-borrow rules for managers yielding references |
| Generics | Explicit type parameters constrained by structural `protocol Name:` declarations | Static constraint checking and cached specialization |
| Allocation | Every allocation-capable operation exposes failure; allocation-free error construction | Replace hidden runtime allocations and current boxed error values |
| Allocators | Startup default plus an allocator handle retained by each allocation owner | Provenance, allocator lifetime, and failure-atomic operations |
| Error propagation | Postfix `?`, initially with the same `Result` error type or within `Option` | Correct ownership transfer and cleanup on early return |
| Parallelism | Scoped threads, bounded channels, then explicit parallel collection operations | Thread-safe shared storage and compiler-checked transfer rules |
| SIMD | Explicit vector operations and contiguous buffers, starting with a narrow target contract | Backend support, scalar semantics, bounds, and data layout |
| Tutorial | Runnable literate lesson sources with checked outputs generate Markdown | A small extraction/validation tool; existing tests provide the baseline |

## Entrypoints, imports, and names

The suspicion about startup was correct at the research baseline. The compiler wrapped top-level
statements in a generated entry function. A user function named `main` was just
another function; it was not called automatically. Top-level bindings were startup
locals, not globals visible to other functions. That behavior has now been replaced
by explicit `main`, and the tutorial teaches the new contract.

A required binary `main` makes that contract clearer. Recommend a parameterless
`main` returning unit or an `i32` process status. It need not be `pub`. A library
has no required entrypoint, and an imported function named `main` has no special
meaning. Module top levels should contain declarations and imports, with globals
and constant initialization designed separately. Fallible application code can
initially be called from a small `main` that explicitly handles its result; a
`Result`-returning entrypoint can follow once reporting and exit status are defined.

Use familiar absolute `import package.module` and
`from package.module import Name`, including explicit aliases. Imported names and
builtins account for everything a file can reference beyond its own declarations.
Start with one explicit source root, no ambient path search, no executable package
initializers, no wildcard imports, and no import cycles. These restrictions keep
resolution and initialization easy to understand and compilation inexpensive.

`pub` should govern access to functions, types, fields, and methods. Public classes
do not automatically expose all fields or methods. An automatically generated
field constructor should be public only when the class and all its fields are
public; otherwise construction needs an accessible initializer or factory.
Compiler-invoked destruction must still work for private destructors.

For a simple initial rule, privacy can be module-based, including class members.
Class-private access could be added deliberately if wanted. Privacy controls
source access; today's structural printing and equality mean it is not a promise
that representation details are secret. Public signatures also need checking for
accidental exposure of private types.

Detailed resolution, visibility, and migration rules are in
[Entrypoints, modules, and executable tutorials](entrypoints-modules-and-tutorials.md).

## Structural protocols without import-sensitive behavior

Structural protocols are a good fit. Python's `Protocol` supplies useful precedent
for matching declared capabilities without requiring explicit inheritance; Go's
interfaces offer another precedent. Plenty can make that matching entirely static
for generic calls. Neither requires importing an unrelated trait to make a method
appear. See the [Python protocol specification](https://typing.python.org/en/latest/spec/protocol.html)
and [Go interface types](https://go.dev/ref/spec#Interface_types).

Recommend `protocol Writer:` with typed method requirements, and an explicitly
generic function such as `def save[W: Writer](writer: &mut W) -> Result[(), IoError]`.
This is proposed syntax. A class satisfies the constraint when its accessible
methods match the required receiver, parameters, returns, borrowing, and failure
behavior. Imports only provide names; they never activate methods. An adapter
class can give an external type a different interface when necessary.

Start with protocols as generic constraints, not runtime values. Accepting a
protocol-typed object through a vtable, or storing different implementations in
one list, is a separate feature. This avoids silently introducing allocation or
dynamic dispatch. Unknown generic values remain owned values; generic code cannot
assume copying is free.

Check a generic body against its declared constraints, then specialize reachable
concrete instantiations and cache them. Detect expanding recursive instantiation
and measure generated-code growth. This costs some compile time, but remains a
tractable first design without specialization, blanket implementations, or a large
trait-resolution engine.

The [protocols, generics, and context managers proposal](protocols-generics-and-context-managers.md)
contains the proposed matching and specialization rules.

## Context managers and `?`

Concrete context managers can precede generics. The compiler already knows a
concrete class's methods and can check a `with` operation directly. Returning a
reference from `__enter__` does require extending today's borrowing rules: a hidden
owner must remain alive, the yielded borrow cannot escape it, and the borrow must
end before mutable exit/cleanup access.

The essential behavior is once-only cleanup on normal completion, `return`,
`break`, `continue`, and propagated failure. After successful entry, body locals
are cleaned up, `__exit__` runs, then the hidden manager is dropped. An entry
failure must not call `__exit__` as though entry had succeeded. Defer yielding from
inside a context until suspended-generator cleanup is specified.

Start with infallible `__exit__ -> ()`, and make operations that can fail visibly
explicit: acquisition before entry, and `flush`, `commit`, or `finish` before exit.
The destructor remains a cleanup fallback. Fallible exit is possible later, but
needs an answer for simultaneous body and exit errors; silently discarding one
would be a poor default. An infallible signature also does not by itself prove
that arbitrary user cleanup never allocates.

Postfix `?` is worthwhile, especially with fallible allocation. Start with direct
propagation: `Result[T, E]` into a function returning `Result[U, E]`, and `Option[T]`
into `Option[U]`. Avoid implicit error conversions or cross-family propagation.
Evaluate the operand once, move its payload, and perform the same cleanup as an
explicit early return. This can be implemented against today's sums first, but it
must not be described as OOM-safe until error propagation itself is allocation-free.

## C libraries and trusted interfaces

One correction to the premise: Rust does support C shared libraries, exporting
`cdylib` libraries, and runtime loading. Dynamic linking is possible; the hard part
is establishing the lifetime, aliasing, ownership, and callback contracts that the
C ABI does not express. Rust documents its [library linkage forms](https://doc.rust-lang.org/reference/linkage.html),
and [libloading](https://docs.rs/libloading/latest/libloading/) supports runtime loading.

Plenty can make that work more convenient with reusable interface files. Separate
the raw native signature from a normal Plenty-facing API, and generate or write
adapters between them. The interface is a trusted contract maintained once by a
binding author. Callers can then use ordinary ownership, borrowing, and `Result`
without repeating low-level boundary work at each call.

Those contracts need more than `const`: buffer lengths, nullability, retained
aliases, borrowed lifetimes, ownership transfer on success and failure, the exact
release function, error reporting, callback retention, and thread restrictions.
Annotations cannot prove that arbitrary native code honors its promises; relaxing
the rest of Plenty's ownership rules would not solve that problem.

Cython's declaration files and Zig's explicit C ABI/layout facilities are useful
models. C-header translation could eventually be a separate tool, with compiler
flags and target information, but cannot infer all semantic contracts. It should
not be a prerequisite for a small, useful initial FFI.

Keep Plenty's `str`, class, collection, and enum representations private. Begin
with C scalars, opaque handles, and explicit pointer/length or C-string adapters.
Retain the matching allocator and release function for owned foreign resources.
Opt-in C-layout records can follow. `pub` visibility must remain separate from a
stable C export declaration.

Ordinary shared-library linking and runtime loading should be separate stages.
Known signatures can be called indirectly after runtime loading with AOT code;
this does not require reviving the JIT. Initially retain loaded libraries for the
process lifetime, and defer retained or foreign-thread callbacks until their
ownership and synchronization rules exist.

The [FFI and dynamic libraries proposal](ffi-and-dynamic-libraries.md) documents
the contract fields, current backend assumptions, and staged delivery plan.

## Recoverable allocation is an architectural commitment

Rust already has fallible allocation-related operations such as
[`Vec::try_reserve`](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.try_reserve).
Plenty's opportunity is to apply a consistent recoverable policy across its own
language and runtime operations.

Today's runtime cannot make that promise. `Result` and `Option` are managed,
heap-allocated sum values. Runtime type metadata, collection rebuilding, strings,
generators, and some structural equality operations also allocate. Even string
indexing and range construction need auditing. An allocation failure cannot
reliably be reported by attempting another allocation to construct `Err`.

First establish allocation-free finite sum/error lowering and static type
metadata. Keep that lowering contract internal and evolvable. Then audit every
allocation-capable operation, including cleanup and propagation. A small
`AllocError` must be representable without allocating or formatting a message.

Fallibility belongs to an operation's static interface even when a particular
call happens to fit existing capacity. Literals, comprehensions, concatenation,
copying, class construction, and generator creation all need an explicit policy,
not just `append`. Whether their syntax yields `Result` directly or uses an
explicit allocation effect remains an open design choice; hidden automatic `?`
would undermine the desired visibility of failure.

Failed mutations should preserve existing logical contents, and failed builders
must release partially constructed results. Decide how failed insertion handles
an owned input: consuming and dropping it is simple; returning it with the error
needs a deliberate recovery API. Test every allocation position with fault
injection, including attempts to report and clean up after failure.

Allocator selection is feasible without exposing a different `list` type for
every allocator. Recommend a default chosen at startup and an explicit constructor
argument for custom allocation. Each owner retains allocator provenance and keeps
the allocator alive until deallocation. Nested values retain their own allocation
provenance; inserting them does not magically relocate them. Resettable arenas
need additional lifetime constraints and should come later.

Fixed-capacity containers and inline-small containers are both useful but have
different guarantees: the latter can still allocate after spilling. Neither needs
to complicate the initial ordinary collection type. Budgeted allocators would
make predictable service-level refusal practical, although operating-system
overcommit means no language can guarantee recovery from every physical-memory
exhaustion event.

The [memory, parallelism, and SIMD proposal](memory-parallelism-and-simd.md) gives
the allocation audit, failure guarantees, allocator design, and sources.

## Parallelism and SIMD

Owned values make concurrency easier to reason about, but moving a collection
does not prove all its storage is exclusive. Shared immutable strings and runtime
metadata currently involve non-atomic reference counting. Allocators, destructors,
and foreign handles can also have thread affinity. These need compiler-recognized
transfer/share rules and runtime changes before safe thread APIs.

Start with scoped threads that join on every exit, bounded channels with defined
failure ownership, and explicit parallel map/collect/reduce operations using a
configured pool. Define result order, error selection, cancellation, and cleanup
before implementation. A thread-count setting should initially affect explicitly
parallel work only.

Automatic parallelization is a later optimization requiring proof that observable
behavior is preserved, including allocation failure, side effects, destruction,
and floating-point reduction order. Structural method matching alone cannot prove
those properties.

SIMD should have its own narrow, testable contract. Explicit small vector values
and contiguous numeric buffers are a reasonable first step; current generic list
storage is not automatically a contiguous native numeric array. Begin with
operations the selected Cranelift targets can implement, define scalar-equivalent
fallbacks and tail handling, and leave CPU dispatch and wider target-specific
vectors for later. Do not promise that all backends support identical native
instructions or that floating-point reassociation is invisible.

## Runnable lessons and generated documentation

The proposed documentation workflow is workable. The tutorial already has direct
executable coverage: 43 successful examples and 11 rejected examples. Its test
checks output through both compile-and-run and explicitly compiled executables,
and checks expected diagnostics for rejected examples. That test passed after the
startup clarification in this change.

Standalone `.plenty` lessons would improve editing, discovery, and reuse. Give
each lesson a complete runnable program, prose in ordinary comments, and inert
markers selecting displayed code regions. An ordered manifest selects lessons;
sidecars can hold expected output or diagnostics. Show the complete `main` in the
first example and label later shortened snippets as excerpts with links to the
full source.

The documentation tool should run and validate the whole program against reviewed
expectations before generating Markdown. It must not silently turn changed output
into a new expected result. CI should check that the committed document matches
the generated version. Compile-fail lessons and future multi-file module examples
need first-class support too.

TinyTemplate can render the final document, but extraction, execution, validation,
and ordering are separate work. Its default HTML escaping needs disabling for
trusted Markdown. A simple renderer may be sufficient; the important design is a
single authored source and independently checked behavior. Details and TinyTemplate
references are in the [tutorial workflow proposal](entrypoints-modules-and-tutorials.md).

## Work tracking

The original cross-feature implementation sequence has been retired. The
[backlog](../backlog.md) is the single maintained list of remaining work and
proposed order; the [implementation status](../design/04-implementation-status.md)
records the result. The sections above preserve the design reasoning and open
questions from this review, not a current task queue.
