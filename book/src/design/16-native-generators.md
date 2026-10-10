# Native generators

Owned generators can travel through `Option` and `Result`, including `?` and
pattern matching. These wrappers remain affine and non-copyable; dropping one
releases its frame and captures without resuming it. Wrapping does not permit
generators in collection/class storage, printing, comparison, or nested yields.
References remain prohibited in standard sum payloads.

Use ordinary call syntax to construct a generator: `generator_function(arguments)`
returns the generator directly. The `.new(arguments)` spelling remains an
equivalent alias. Frame construction needs no heap allocation and does not run
the body. Arguments evaluate first and move into the frame; their own allocation
and error contracts still apply. Frame destruction needs no allocation and does
not resume the body.

Each generator body and specialization has a distinct concrete frame type,
one emitted resume function, and one immutable frame descriptor.
`Generator[T]` in source describes the yield contract. Local annotations preserve
the initializer's concrete type. Parameters, including references and standard
sum wrappers, specialize by the actual frame types supplied at the call site.
Aliases obey the same rules, and explicit generic type arguments remain optional
when inferred from arguments.

A factory returning `Generator[T]` must resolve to one concrete producer per
specialization; different producers cannot join merely because they yield the
same type. Standard sum payloads each retain their own concrete identity.
An empty-only factory cannot infer a producer from `Nothing` or `Err` alone.
No hidden boxing or size-erased owned generator is introduced.

A function containing `yield` declares `Generator[T]`. Calls evaluate arguments
and create an owned frame without running the body. Each resume executes native
code until a statement-only `yield value`, bare `return`, or fallthrough.
Yield types are exact; owned payloads transfer into the yielded value.
Nested generator yield types, reference yields, and unit are
rejected. Generator functions cannot return a value. An ordinary factory without
`yield` may return another generator by moving it.

`next(&mut g)` borrows a mutable generator binding or reborrows an exclusive
generator reference, and returns `Option[T]`. The shorthand `next(g)` remains
supported for named bindings and exclusive references. Shared references and
immutable owners cannot be advanced; mutable temporaries are rejected.
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
Completion clears owned slots and marks exhaustion. Dropping any state releases
remaining captures without resuming the source body.

Ordinary slots use 16 bytes. A range or standard sum containing a range adds a
32-byte inline payload, so ranges captured or created in a suspended frame do not
refer to expired caller/resume storage. Yielded ranges copy into caller-provided
storage. Generator slots reserve their complete concrete payload after the
ordinary value bits. Standard sums reserve the largest possible inline payload
among their variants. Frames can contain other generator frames.

Constructors and returning functions use caller-owned output storage. Moves
relocate nested inline payloads without allocating, and cleanup finishes
synchronously before the enclosing storage expires, including inside drop hooks.
Calls passing or returning inline storage (or references to it) use ordinary
calls when a native tail call could invalidate that storage.

Frame layouts are cached per concrete producer. Recursive inline layouts are
rejected; nested layouts are limited to 64 levels and native frame offsets to
signed 32-bit sizes. Consumer specializations are cached and limited to 256 per
compilation. Factories whose return annotations contain unresolved frames are
checked again with the inferred return context, so empty sum variants and early
propagation use the same layout on every path.

Integration deliberately reuses the checked structured operation tree instead
of introducing a second source IR in this batch. `Yield` requires an empty
residual operand stack. Native lowering adds resume-dispatch edges to continuation
blocks; all generator locals are frame-backed, so no SSA value needs to survive
between invocations. Cranelift verifies the resulting CFG. This is native
state-machine lowering, with no interpreter, C-stack suspension, or eager yield
collection. The initial state dispatch is a linear comparison chain.
Iteration wraps each resume result in an allocation-free inline `Option`.
Optimizing frame liveness remains a later runtime improvement.
