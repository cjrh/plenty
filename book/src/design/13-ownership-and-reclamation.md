# Ownership and reclamation

Classes, mutable collections, and aggregates containing them move on assignment and owned
argument passing; independent duplication requires a successful `copy(value)`. Immutable `str`
and enums containing only immutable values may share storage. Copyability is
cached on concrete enum and class metadata so shared type graphs are not traversed repeatedly.

Every managed expression operand, live local, stored field, and frame capture
has one owner. Loading a copyable value retains its immutable storage. Moving
an owned value clears the source ownership slot. Stores evaluate the RHS, release the
old owner, then transfer the new one. Scope exits, loop exits, and function exits
release locals; compiler-private temporaries are bounded by local slots.
Tail-call arguments are owned before caller cleanup. Observable resource cleanup
prevents tail-call rewriting in functions with resource-bearing slots. Traps terminate the process without unwinding language scopes.

Runtime objects share `{atomic u64 refs, destroy_callback}`. Heap objects start with one
reference; literal strings use an immortal count. Helpers borrow arguments and
return owned managed results, including retained projections and builder aliases.
Buffers have explicit owners too; type metadata is immutable program data. Destruction uses an
iterative queue, avoiding recursive C-stack growth through owned value graphs.
Retain uses relaxed atomic updates; final release acquires preceding releases
before destruction. The final thread owns the destruction queue entry. Inline
generator release uses the same count handoff but finishes synchronously.
This protects ownership counts; it does not permit concurrent mutable payload
access or make foreign resources transferable. The language still has no thread API. The current
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
A generator cannot be copied or stored in collections, classes, or user enums;
standard `Option`/`Result` wrappers can own it. Collection/enum payloads
can be owned mutable values: construction transfers ownership, and consuming
matches transfer their bound payloads. Enums with such payloads are also affine.
