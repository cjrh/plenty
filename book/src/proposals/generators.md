# Native owned generators

> Active tasks and priorities live in the [backlog](../backlog.md). Staging below
> records design dependencies, not scheduled work; some syntax and assumptions
> are historical. See the [implementation status](../design/04-implementation-status.md)
> and [reference](../design/index.md) for current behavior.

Status: implementation proposal. This document specifies the first generator
slice; it does not claim generators are implemented. Reconcile accepted behavior
into `DESIGN.md` and runnable examples into `TUTORIAL.md` when it lands.

## Source contract

Use the compiler-known concrete type `Generator[T]` and statement-only `yield`:

```python
def countdown(start: i64) -> Generator[i64]:
    mut remaining = start
    while remaining > 0:
        yield remaining
        remaining = remaining - 1

for number in countdown(3):
    print(number)
```

Calling a generator evaluates its arguments immediately, transfers those values
into a new frame, and returns without executing its body. The first resume starts
the body. Each resume runs until the next `yield`, bare `return`, or fallthrough.
Each yield evaluates its operand exactly once and produces one owned value of
the declared element type. There is no implicit yield from a tail expression.

A function containing `yield` is a generator function and must declare
`Generator[T]`; ordinary functions may return that type by returning another
generator. Determine whether a function contains a yield syntactically before
checking its body, including yields in unreachable branches. No nested function
syntax currently complicates this scan. An empty generator can contain
`if False: yield value` (written as the usual indented suite). Do not turn every
ordinary factory returning `Generator[T]` into a suspended function.

`yield` requires an expression of exactly `T`. A generator's `return` accepts no
expression and means completion; `return value` is an error even if `value` is
itself a generator. Falling off the body also completes it. The initial stored
type restriction excludes `Generator[()]`, consistently with the enum proposal's
unit-payload restriction. Reject yielding at module scope, yielding in a function
with a different declared result type, and nested generator element types.

Only value-copyable types may be yielded initially: scalar values, the one `str`
type, current collections, and concrete enums whose fields are value-copyable.
Yielding a managed local retains an independent value; it does not consume that
local. A generator may own another generator as a parameter or local, but may
not yield it. `yield from`, `send`, exception injection, generator expressions,
closures, async/await, and public references are outside this slice.

## Affine ownership and explicit advancement

`Generator[T]` is an affine owner of one advancing execution state. Assignment,
argument passing, and return move it. Reading the old binding afterward is a
compile-time error. `mut` controls replacing or explicitly advancing a binding;
it does not make a generator copyable. Ordinary strings and collections retain
their existing independent-value behavior.

```python
mut numbers = countdown(2)
match next(numbers):
    case Option[i64].Some(number):
        print(number)
    case Option[i64].Nothing:
        print("finished")
```

`next` is a compiler-known operation on a named mutable `Generator[T]` binding.
It temporarily borrows that owner exclusively and returns `Option[T]`. The
borrow ends before subsequent source expressions execute and cannot escape into
a value. This restricted operation needs no public reference syntax, lifetime
parameters, user traits, or general borrow inference. Initially reject
`next(countdown(2))`, `next` on an immutable binding, and non-generator operands
with specific diagnostics. Users can name a mutable binding or use a `for` loop.

Exhaustion is stable: every later `next` returns `Nothing`, without rerunning the
body or double-dropping locals. A generator that has completed remains an
available owner until moved or dropped. Dropping an unstarted or suspended
generator releases its captures and locals without executing any remaining user
statements. No implicit user finalizer or exception-unwinding behavior is added.

Reject generators recursively inside list/dict/set elements, enum payloads, and
Option/Result arguments in this slice. Those types currently copy projections,
so admitting an affine field would require aggregate moves and consuming match
semantics. Aliases must not bypass this restriction. No equality, ordering,
hashing, collection indexing, or `len` is defined on a generator. Printing a
generator should diagnose an unsupported printable type rather than expose its
address or consume it.

Use the ownership proposal's `Available`/`Moved`/`MaybeMoved` analysis on stable
local IDs. Join only continuing paths. Every reachable loop backedge, including
`continue`, must preserve availability of outer owners available on entry;
move-then-reinitialize is valid, an unreinitialized move is rejected. Break paths
join the zero-iteration path after the loop. This is deliberately conservative
and should be described as such in diagnostics, not sold as a complete borrow
checker.

## Iteration consumers

`for item in generator` evaluates the iterable once and moves it into a hidden
owner whose lifetime is the loop. Each condition check resumes once. A yield
initializes the loop binding and enters the body; exhaustion exits. `continue`
goes to that resume condition, and `break` destroys the hidden owner immediately.
Returning from the consumer also destroys it. Consuming a named generator leaves
the original binding moved even when it yields no values or the loop breaks on
its first item. `for` does not require the source binding to be mutable because
ownership is transferred to the compiler's iterator binding.

Comprehensions and currently supported iterable constructors use the same
adapter. For example, `[n * n for n in countdown(4) if n > 1]` remains eager as a
list construction but obtains its input lazily. Preserve existing left-to-right
clause evaluation and filter behavior. Creating an inner generator expression by
calling a generator function happens each time that clause is entered; trying
to repeatedly move the same outer generator binding across an outer loop's
backedge is rejected. Do not introduce Python's tuple-producing dictionary
iterators or a generic iterable protocol as a prerequisite.

Refactor the current collection iteration helper into a private iteration plan:
setup operations, condition, item-binding prefix, step, and cleanup. Existing
collections/ranges keep their snapshot and index step; strings can use the byte
cursor proposed in `strings.md`; generators have an empty step because resume
belongs in their condition. This removes the current assumption that every
iterator has an integer index to increment. Every consumer must emit the same
step for normal progress and `continue` and the same cleanup for exhaustion,
`break`, return, and nested scope exits.

The simplest initial generator adapter calls the typed `next` helper into a
hidden `Option[T]` local. Its condition tests `Some`; its body extracts an owned
payload and drops the option owner before running user statements. Its exit
cleans the final `Nothing` value and generator frame. This reuses checked enum
operations and avoids inventing a special two-result source operation. A later
optimization can use the native ready flag and output slot directly, without
allocating one option record per iteration.

## Native frame and callback ABI

Preserve one pointer-sized native value for `Generator[T]`. The frame begins
with the shared managed header and stores a concrete resume callback:

```c
typedef uint8_t (*PlentyResume)(void *frame, uint64_t *out);

typedef struct PlentyGenerator {
    PlentyObject object; /* { uint64_t refs; void (*destroy)(void *); } */
    PlentyResume resume;
    uint64_t state;
    uint64_t running;
    /* Concrete, aligned parameter and local slots follow. */
} PlentyGenerator;

uint8_t plenty_generator_resume(PlentyGenerator *frame, uint64_t *out);
```

The compiler emits constructor, resume, and destructor functions for each
generator function, once per concrete source signature. Constructor and resume
callbacks must use signatures/calling conventions compatible with the C runtime;
the callback ABI is an internal implementation contract, not an FFI promise.
The constructor allocates one concrete frame, zeroes its managed ownership slots,
moves parameters into their slots, sets entry state, and returns an owned frame.
It executes no body operations. Affine frames are never retained by source copy
operations even though they share the common destruction header.

`resume` borrows the frame exclusively. Returning `1` writes one owned value to
`out`; returning `0` does not initialize `out`. Pack integers and pointers with
the same rules as collection slots. The caller must never read or release `out`
on the completed path. A helper checks the completed state and a running flag,
calls the concrete resume callback, and clears the flag. The guard diagnoses an
invalid reentrant resume rather than corrupting the frame; public callbacks or
concurrency are not introduced by this guard.

A generated typed wrapper converts this protocol into the existing concrete
`Option[T]` constructors for source `next` and the initial iteration adapter.
On a yielded path, construction borrows the owned output, retains a stored
managed payload, and then releases the temporary output owner. On the completed
path, it constructs `Nothing`. Keep this wrapper with enum-aware lowering so
the base runtime does not acquire an unconditional dependency on enum allocation
or metadata. The wrapper borrows, rather than consumes, the generator owner.

Assign dense continuation states in source control-flow order; reserve entry
and completed states. Each `yield` writes the successor state, transfers the
yielded owner to `out`, and returns `1`. Completion releases live frame owners,
clears their slots, records completed state, and returns `0`. The generated
destructor releases remaining non-null owner slots in reverse scope/declaration
order and frees the allocation. The same destructor handles unstarted,
suspended, and already completed frames without needing a distinct callback per
suspension point. Internal null ownership slots are not nullable source values.

## Typed control flow without a coroutine runtime

Do not suspend the C stack, keep Cranelift SSA values across native calls, use
`ucontext`/threads, interpret the body, or collect all yields eagerly. Flatten the
typed structured body into explicit basic blocks for the generator resume body.
Keep the existing structured operation path for ordinary functions initially.

Introduce a small typed generator CFG with source spans, local IDs/types,
ordinary operations, and terminators for jump, branch, match, yield, and complete.
Flatten `Op::Loop` into header/body/exit, preserving the existing explicit step
operations emitted before `continue`. Translate `Break`/`Continue` using an
innermost-loop target stack, and translate `Op::Match` arms and joins directly.
This also gives ownership checking and later borrow analysis a reusable place
to record edge cleanup, without requiring a new optimizer or immediate migration
of every non-generator function.

Require `yield` to leave an otherwise empty expression operand stack. As a
statement, its argument is completely evaluated and consumed before suspension.
The resume CFG can then split at yields without spilling partial expression
evaluation or callee stack frames. Source calls remain ordinary native calls;
only a call to a generator constructor returns a suspended frame.

For the first implementation, give all generator locals frame-backed slots,
including hidden iterators, builders, parameters, and shadowed locals. This
avoids liveness analysis, SSA reconstruction across resumes, and error-prone
copy-in/copy-out ownership. A managed local load retains an expression owner;
a move clears its slot; a store releases the old value and transfers the new
owner. Scalars use the same frame storage without reference operations.
Control-flow definite-initialization checking still rejects source reads from
uninitialized locals. Managed slots start internally null for safe cleanup on
paths where their declaration never executes.

Emit a state dispatch at resume entry to the entry or saved continuation block.
Emit all frame loads/stores in that invocation normally; local expression SSA
values need survive only until the current yield. Scope-exit cleanup happens on
normal edges, breaks, continues, and returns before their destination. A yield
is not scope exit: active owners remain in the frame. Locals dead across a yield
may remain in fixed frame slots initially; frame-size/liveness optimization is
a later improvement. This is finite per-generator storage, not accumulating one
native stack frame per yielded item.

The independent checker must validate yield type, empty residual operand stack,
frame slot type/ownership, valid continuation IDs, block-entry operand shapes,
and initialized local use. It must reject affine duplication and a yield/complete
terminator outside a generator body. It should validate all edges, including
resume entry edges, rather than assuming structured source alone proves safety.
Generated scope cleanup and definite-move facts must survive CFG conversion.

Calls from a generator before a yield are not tail calls: its frame must remain
available for later resume. Ordinary functions forwarding a newly constructed
generator can retain existing tail-call behavior. Do not disable ordinary direct
or mutual tail calls merely because generators exist in the same module.

## Implementation sequence

1. Finish concrete Option/enums, common managed headers, complete value ARC, and
   deterministic scope cleanup. These are correctness dependencies for frames,
   not requirements for Rust-like references or a trait system.
2. Add `Generator[T]`, recursive placement validation, syntactic generator
   classification, typed yield/bare-return diagnostics, and affine place analysis.
3. Add the typed resume CFG and independent checker. Test flattening of nested
   loops, match exits, and source locations before connecting native frames.
4. Add frame layout, constructor/resume/destructor emission, runtime dispatch,
   and typed `next` wrapping. Keep frame locals in memory initially.
5. Refactor the private iteration plan and wire generator `for`, comprehensions,
   and supported collection constructors through it. Preserve existing snapshot
   iteration, string behavior, and loop control tests.
6. Add runnable tutorial lessons and mark only delivered behavior implemented in
   DESIGN. Record lazy effects, affine consumption, stable exhaustion, and the
   deliberately unsupported features explicitly.

## Acceptance tests

- Construction performs argument side effects once but no body side effects;
  first and subsequent resumes perform only effects before the corresponding
  yield. Drop of an unstarted or suspended frame never executes later prints.
- Zero yields, conditional yields, multiple sequential yields, yields in nested
  loops/match arms, early completion, and repeated exhausted `next` all behave
  correctly. A long generator uses bounded native stack depth.
- Iteration evaluates its source once; nested generators, filtered and nested
  comprehensions, break/continue, and consumer return resume the right frame and
  destroy only owners leaving scope.
- Strings with embedded NUL, nested collections, and enum payloads survive yield
  and subsequent frame mutation/destruction. Updating a yielded collection does
  not change the frame's retained independent value.
- Diagnose use after assignment/call/return/iteration moves, one-branch moves,
  repeated moves across backedges, immutable `next`, unknown element types,
  generator-valued yields, forbidden aggregate placement through aliases, and
  valued returns from generator bodies. Accept move followed by reinitialization
  and exclude returning branches from joins.
- Test frame destruction before first resume, at every suspension state, after
  full exhaustion, after `break`, and after consumer return. Include a frame
  owning another frame and shadowed managed locals on mutually exclusive paths.
- Test dynamic allocation/drop counts plus ASan/UBSan where available; correct
  printed output alone does not catch leaks, duplicated ownership, invalid out
  slot reads, or double release after exhaustion. Static literals/metadata are
  excluded from dynamic counts.
- Keep legacy AOT, existing non-generator tail calls, all executable tutorial
  examples, formatting, Clippy, and the full suite passing. Track compilation
  cost on a function with many yield points; generated code/frame metadata must
  grow linearly with that concrete body's size.
