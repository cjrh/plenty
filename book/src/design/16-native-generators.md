# Native generators

Owned generators can travel through `Option` and `Result`, including `?` and
pattern matching. These wrappers remain affine and non-copyable; dropping one
releases its frame and captures without resuming it. Wrapping does not permit
generators in collection/class storage, printing, comparison, or nested yields.
References remain prohibited in standard sum payloads.

Use ordinary call syntax to construct a generator: `generator_function(arguments)`
returns `Result[Generator[T], AllocError]`. The `.new(arguments)` spelling remains
an equivalent alias. Arguments evaluate first and move into the
constructor. Allocation failure releases them; success transfers them into the
frame without executing the body. There is no public aborting frame constructor.
Frame destruction remains allocation-free and does not resume the body.

Generator construction uses one emitted resume function and one immutable frame
descriptor; selecting the checked constructor never duplicates the body.

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
owns parameters and all locals in typed slots, plus immutable slot-type metadata,
resume callback, continuation state, and reentrancy guard. Resume has the internal
C ABI `(frame, out_slot) -> ready`; successful yields transfer an owned value.
Completion clears owned slots and marks exhaustion. Dropping any state frees
remaining captures without resuming the source body.

Ordinary slots use 16 bytes. A range or standard sum containing a range adds a
32-byte inline payload, so ranges captured or created in a suspended frame do not
refer to expired caller/resume storage. Yielded ranges copy into caller-provided
storage. Generator frames themselves still allocate.

Integration deliberately reuses the checked structured operation tree instead
of introducing a second source IR in this batch. `Yield` requires an empty
residual operand stack. Native lowering adds resume-dispatch edges to continuation
blocks; all generator locals are frame-backed, so no SSA value needs to survive
between invocations. Cranelift verifies the resulting CFG. This is native
state-machine lowering, with no interpreter, C-stack suspension, or eager yield
collection. The initial state dispatch is a linear comparison chain.
Iteration wraps each resume result in an allocation-free inline `Option`.
Optimizing frame liveness remains a later runtime improvement.
