# Returned references

A function or method returning `&T` or `&mut T` must take exactly one reference
parameter. A mutable result requires a mutable parameter. Every return path must
produce a borrow originating in that parameter; local owners and owned value
parameters cannot escape. No lifetime syntax or whole-program inference is needed.
Direct parameter references, reborrows, field projections,
[borrowed match payloads](31-borrowed-enum-matching.md), and forwarding calls
are supported, including branches with explicit returns. Conditional reference
expressions are not supported yet. Reference-returning methods require a named
receiver, class-field place, or indexed place in named storage; temporary
receivers are rejected because their owner cannot outlive the returned loan.

Callers may bind the result to an immutable reference binding and reborrow it.
The result extends the input loan until its last use. A function whose entire
body directly returns a parameter or a fixed field projection has a cached field
summary. Its returned loan protects only that projection, allowing subsequent
access to disjoint fields. Other bodies, indexed origins, and control-flow-dependent
returns conservatively protect the entire borrowed argument even after further
projection. Summaries do not narrow the access needed to make the original call.
References cannot be stored
in aggregates or retained across generator suspension. Reference returns keep
normal calls where frame or cleanup lifetime requires them.
