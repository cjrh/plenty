# Owned structures and checked node identities

> **Status: Investigation complete.** Owned-chain and tree-rotation examples,
> an arena lesson, and a checked-source prototype exercise existing Plenty.
> No `std.arena` API or new borrowing capability has shipped. The
> [recursive-storage](../design/language/03-types/recursive-data.md),
> [borrowing](../design/language/05-ownership/02-borrowing.md), and
> [box](../design/facilities/01-builtins/boxes.md) references own current behavior.
> B73 in the [backlog](../backlog.md) owns arena delivery; B11 owns broader
> borrowed traversal and collection-loan precision.

## Make the correct representation easy to use

The goal is that a safe, efficient implementation should also be the obvious
and simplest implementation for its workload. Memory safety alone is not enough:
failure can destroy data, stale indices can identify the wrong object, and a
recursive traversal can exhaust the native stack even when automatic cleanup
is bounded-stack.

The investigation distinguishes uniquely owned chains/trees from parent links,
shared subgraphs, and intrusive collections. Direct box field access reduces
syntax but does not change their ownership requirements. A generic arena is a
better first library candidate than separate general-purpose list, tree, and
graph APIs: it centralizes identity, deletion, and checked access without fixing
application-specific edge semantics.

## Evidence and limits

The examples and `tests/test_data_structure_patterns.rs` preserve these findings:

| Experiment | Result |
| --- | --- |
| Owned linked stack | Fallible push, ownership-transferring pop, borrowed iteration, and mutable iteration work. |
| Tree rotation | Existing boxes can be rewired without allocating, including transfer of the pivot's middle subtree. |
| Deep chain | One million nodes traversed, mutated, and explicitly dropped with allocations disabled under a 256 KiB native stack on Linux. This is a regression check, not a throughput benchmark. |
| Failed insertion | Allocating an empty node before linking preserves the existing chain. Detaching into a fallible box constructor does not. |
| Arena links | Indices express back/parent links and shared targets without ownership cycles. The TOML parser already uses this representation. |
| Borrowing limits | Stored references, including a worklist of borrowed tree nodes, are unsupported. Different list indices do not establish disjoint mutable loans. |

Bounded-stack cleanup is a useful semantic advantage, but recursive user calls
and destructor hooks still consume native stack. The current box runtime carries
a 32-byte header before its payload; simpler syntax is not evidence of lower
per-node memory cost. No comparative timing benchmark was performed.

For ordinary stacks use a list; for queues use the existing
[`std.deque`](../design/stdlib/deque.md). Its growth already preserves existing
contents on failure. Keep owned chains and tree rotations as
[teaching examples](../design/examples/data-structures/index.md) until an
application motivates a stronger packaged collection API.

## Candidate arena contract

A candidate `std.arena` would own node values and issue typed handles. Handles
identify nodes; they neither own nodes nor keep them alive. The arena validates
identity before granting temporary access. Deletion invalidates the handle;
slot reuse must not make an old handle identify a replacement.

- Typed handles distinguish payload types. Arena identity must also distinguish
  separate instances with the same payload type, including after destruction.
- Growing storage preserves logical IDs but does not guarantee stable addresses.
- Reusing vacant slots changes a generation. Exhausted slots retire permanently
  rather than wrapping. Arena-identity exhaustion needs an equally explicit rule.
- Failed insertion preserves all existing elements and handles. The initial
  candidate consumes the incoming value on failure, following `Deque.append`;
  an ownership-returning variant is a separate contract to evaluate.
- Lookup and removal reject invalid/stale handles through a checked result.
  They do not trap just because a handle has expired.
- Node removal does not silently rewrite inbound edges. Applications either
  remove those edges or handle a failed lookup. Unreachable nodes remain owned
  until removed or the arena is dropped.
- Safe access to two nodes rejects equal or invalid handles before invoking the
  callback. A callback error restores storage ownership, but does not roll back
  mutations already performed by the callback.

Returning `Option[&T]` or a tuple of mutable references is not an available
implementation strategy today. Scoped callbacks returning owned results fit the
current storage rules. Their usability and move costs need evaluation before a
public API is frozen. A separate language change could eventually offer direct
disjoint loans; the library should not require one merely to demonstrate value.

## Prototype

The [prototype assessment](arenas/prototype.md) describes the executable module
in `examples/arena/arena.plenty`, its API, validation, and deliberate namespace
limitation. It is ordinary Plenty and is not embedded under `std`.

This distinction matters: requiring callers to invent unique namespace numbers
would move a correctness problem into application code. The prototype proves
generation validation and scoped access, not a satisfactory identity issuer.
A production constructor needs automatically unique identity within a documented
domain. A non-reusing runtime token is one possible narrow primitive; an owned
issuer with enforced domain separation is another. Concurrency, copying,
serialization, and exhaustion determine which contract is sound and convenient.

[`std.atomic`](../design/stdlib/atomic.md) and
[`std.sync.Mutex`](../design/stdlib/sync.md) now provide synchronized mutation
through scoped shared borrows. An atomic counter could serve one identity issuer,
but separate counters can issue the same numbers: issuer uniqueness and lifetime
remain part of the arena contract. Atomics do not by themselves provide shared
ownership of an issuer or arena.

Issue #326 separately explores a counted shared owner for user data and a
thread-shared HTTP pool. Its eventual access, cycle, and destruction rules may
offer another representation for shared nodes or a shared identity issuer.
These examples and the arena prototype do not depend on that design, and do not
choose its cycle policy. Reassess the representation guidance when that feature
ships; retain arenas for workloads that need checked logical identity and
explicit removal independently of shared-owner lifetimes.

## Rust and intrusive collections

Rust also supports safe owned singly linked lists and trees. It already provides
field autodereferencing, so Plenty's omitted dereferences are a modest ergonomic
difference. Recoverable allocation is another difference: Plenty's box constructor
returns `Result` and destroys its input on failure. Compare failure contracts,
not only the shortest successful-path expression.

An intrusive collection embeds links in application objects, potentially for
multiple memberships. Pointer-based implementations can need stable addresses
and lifetimes governed outside the collection. These requirements differ from
an ordinary owning doubly linked list. An arena can emulate many link topologies
while changing physical-address and lifetime semantics. It is not a replacement
for all pointer-intrusive use cases.

Safe libraries can encapsulate trusted implementation machinery even when the
source language has no `unsafe` keyword. Add such machinery only when a concrete
workload needs stable-address/external-lifetime semantics. There is no evidence
here about which kind of list programmers complain about most.

Relevant precedents, consulted for the original investigation:
[Rust linked-list guidance](https://doc.rust-lang.org/std/collections/struct.LinkedList.html),
[checked disjoint slice access](https://doc.rust-lang.org/std/primitive.slice.html#method.get_disjoint_mut),
[slotmap's versioned keys](https://docs.rs/slotmap/latest/slotmap/), and
[intrusive-collections](https://docs.rs/intrusive-collections/latest/intrusive_collections/index.html).

The documentation teaches implemented behavior. This proposal preserves the
representation decisions and alternatives; priorities and delivery work remain
only in B73/B11 in the backlog.
