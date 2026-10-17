# Linear obligations

Status: **Not implemented; proposed design for review.** Issue #149, tracked
under [B72](../backlog.md). No syntax or standard-library conversion is approved.
The current contracts remain [ownership](../design/language/05-ownership/01-bindings-moves-and-copies.md),
[destruction](../design/language/05-ownership/06-destruction.md), and
[error propagation](../design/language/08-errors/01-sums-and-propagation.md).

This proposal defines an opt-in ownership obligation. Its
[aggregate and error rules](linear-obligations/values-and-errors.md) are part of
the same semantic core. A separate [scope-resolution proposal](linear-scope-resolution.md)
adds automatic synchronous finishing. The [integration contract](linear-integration.md)
compares proposal boundaries, effects, typestate, and future concurrency work.
All code fences in these pages are pseudocode, including names, annotations,
signatures, and outcome types; none are runnable Plenty examples.

## Purpose and guarantee

An affine owner may be consumed **at most once** and may be discarded. A linear
owner must be consumed **exactly once on an ordinary completed execution path**:
resolve its obligation through a permitted terminal operation, or transfer it
to another owner that inherits the same requirement. Reads, borrows, and
intermediate operations are not consumption. They may happen repeatedly.

This makes obligations such as responding to a request or finishing a local
transaction visible across calls and storage. “Run the entirety of a state
machine” means not abandoning an outstanding obligation before an allowed
terminal outcome. It does not mean visiting every state, forcing success, or
proving termination. A retry loop may run forever while retaining its owner.

The goals are to prevent silent abandonment, preserve explicit failure
ownership, and let ordinary affine programs retain their behavior. Non-goals
are async destruction, general effect syntax, state-specialized methods, blanket
stdlib migration, or stronger unwinding. Traps and process aborts remain outside
the guarantee. It cannot establish fairness, deadlock freedom, successful I/O,
knowledge of remote state, or recovery after a crash.

## Baseline checked for this proposal

The reference and implementation were inspected at local commit `82544a4`.
The source locations below are audit pointers, not additional specifications.

| Current behavior | Evidence and consequence |
| --- | --- |
| Classes, collections, captured closures, and aggregates with affine components move; ordinary scope exit may destroy them | Ownership reference above; `src/ownership.rs`. Moving already transfers ownership, but does not forbid abandonment. |
| Cleanup covers return, `?`, break, continue, replacement, and expression temporaries | Destruction reference above. `__del__` cannot return a recoverable error or yield; suspended generator drop does not resume its body. |
| `?` may erase an error into `Failure.Unspecified`, destroying the original | Error reference above; `src/sum.rs`. The unused-Result diagnostic does not cover unused bindings or wrappers and permits explicit drop. |
| Owned `with` calls borrowed `__exit__ -> ()`, then drops its manager | [Resource scopes](../design/language/08-errors/03-resource-scopes.md); `src/frontend/contexts.rs`. This is not yet a consuming resolver. |
| Worker checking follows reachable native effects, concrete callbacks, and destructors | [Effects](../design/facilities/03-concurrency/effects.md); `src/effects.rs`, `src/threading.rs`. It rejects uncertified indirect/foreign calls, including through stdlib helpers. |
| Dropping a future does not cancel or wait; its worker retains execution responsibility | [Futures](../design/facilities/03-concurrency/futures.md), [executors](../design/facilities/03-concurrency/executors.md); `plenty-runtime/src/executors.rs`, especially `run_worker`, `stop`, and `destroy_job`. Cancelled captures and unclaimed results can still be destroyed. |
| SQLite commit/rollback borrow the transaction; a busy commit permits retry | `stdlib/std/db/sqlite.plenty`, `Transaction`; [typestate exploration](typestate-and-owned-transitions.md). Existing fallback rollback is not a new linear contract. |

## Proposed ownership rules

Introduce a discardability capability independent of ordinary copying.
`Discardable[T]` below is a schematic capability name, not chosen syntax.
Existing types remain discardable unless they contain an obligation or opt in
to nominal linearity. A linear value cannot be implicitly copied or duplicated
by `copy`; explicit duplication of a resource must create separately specified
obligations. A borrow is not an owner and does not acquire this obligation.

The checker tracks live obligations through its ownership control-flow graph.
An owned argument transfers responsibility before the callee runs; the callee's
body must itself resolve or transfer it. Returning an owner is a valid transfer,
including in an error. Passing it to an empty function is not an escape hatch:
that function's exit is rejected. Tail calls must resolve or transfer all other
caller obligations before entering the callee.

| Site | Proposed rule |
| --- | --- |
| End of block/function, explicit return, or `?` failure | Every owner in the scopes left must be resolved or transferred. Check the error path separately from success, including pending evaluated operands. |
| Branch join | Check each outgoing path. An obligation cannot disappear on one path; represent conditional remaining ownership explicitly with a sum. Existing moved/possibly-moved availability still governs subsequent uses. |
| Loop backedge / `continue` | Preserve the existing outer-owner availability rule. Resolve iteration-local obligations before leaving their scope; an outer obligation can remain in its slot across iterations, or be transferred and replaced with a new valid owner. |
| `break` and zero iterations | Check the exited scopes and each resulting join. Do not infer that a collection was drained merely because its loop ended. |
| Ignored result, wildcard binding, expression temporary | Reject implicit destruction of an obligation, even when no named binding exists. Observing a temporary does not consume it. |
| Assignment or replacement | Plain assignment cannot destroy an old obligation. An exchange returning the old owner may transfer it; the returned owner must then be handled. Check failure while evaluating the replacement first. |
| Explicit `drop`, aggregate destruction, fallback `__del__` | Require discardability. Memory reclamation alone is not an authorized protocol finish. |
| Ordinary allocation/call failure | Responsibility stays with a live caller owner, returns in the error, or has already been resolved under an explicit contract. It cannot vanish during rollback of partially constructed values. |
| Fatal trap, abort, or nonreturning computation | No unwinding or liveness promise is added. A function which never returns has not completed an ordinary path. |

Borrow checking continues to forbid consumption while a conflicting loan is
live. A mutable borrower cannot silently remove the referent's obligation;
ordinary replacement must leave a valid owner. Resolving a borrowed value into
a different state requires an explicit checked state/storage contract, not an
assumption that existing `&mut` methods can consume their receiver.

## Construction and terminal authority

Nominal opt-in must carry an abstract obligation, not merely rely on having a
linear field. Otherwise unpacking an all-integer `Transaction` could silently
turn it into ordinary values. Conceptually it has a noncopyable **seal** whose
ownership follows transitions; the seal need not occupy runtime storage.

Seals and registry-authorized terminals are this proposal's conservative design
candidate, not prerequisites of every linear type system. An alternative could
let each type's defining module declare terminal operations as part of its
trusted abstraction contract. That is more extensible but expands the authority
to assert meaningful completion. The registry approach makes the initial trust
boundary auditable at the cost of limiting new standalone resource protocols
until a trusted-extension mechanism is designed.

Safe construction introduces that seal only through the type's authorized
constructor. Private fields/constructors prevent fabricated protocol states.
Representation unpacking transfers every field and the seal; it does not
resolve the seal. A safe transition passes it to its successor. An ordinary
consumer, including one in the defining module, cannot discard it.

Terminal elimination needs a small, explicit trusted boundary. Initially the
compiler/runtime contract registry may authorize particular constructors and
terminal primitives, with an ownership/effect contract for **each outcome**.
Safe wrappers must call those primitives or transfer the seal; an empty wrapper
is rejected. Merely naming a function `commit`, marking it consuming, or adding
an empty destructor grants no terminal authority. There is no safe general
`forget`, user-declared empty eliminator, or FFI cast that removes an obligation.

This cannot prove the external truth of a primitive's claim: the implementation
of “response sent” or “local transaction released” belongs to the trusted
resource contract. Incorrect trusted annotations can invalidate the guarantee,
just as incorrect native ownership metadata can today. Review and test that
boundary explicitly. A user-extensible trusted-declaration mechanism remains
open; this proposal does not invent a general unsafe language or let arbitrary
safe libraries mint terminal witnesses. Ordinary libraries can compose existing
certified obligations without new authority.

Field extraction is a real dependency for ergonomic wrappers. Do not assume
current class partial moves exist. The smallest candidate is consuming,
all-fields deconstruction within the declaring module, preserving any nominal
seal and allowed only when no custom destructor must still observe the whole.
General partial initialization, partial moves through borrows, and destructors
on half-moved objects are outside this proposal. Whole-tuple moves and consuming
enum matching can support early experiments without that extension.

## Implementation and conformance outline

First derive discardability and obligation containment recursively in type
metadata, then attach outstanding-owner information to ownership checking and
every cleanup edge. Add capability checks at generic instantiations and native
operations before allowing linear values into existing containers or workers.
Validate return/error ownership and terminal annotations before lowering cleanup.
Only proven discardable storage may reach ordinary generated drop glue.
Audit temporary, constructor, callable, and allocation rollback paths as well as
visible `drop` calls; suppressing a destructor would merely leak an obligation.

This ordering describes semantic dependencies, not a second work queue; B72
remains the planning record. Runtime resource conversions need their own API
decisions and are not included in this documentation change.

| Positive conformance case | Negative case / diagnostic requirement |
| --- | --- |
| Borrow a transaction repeatedly, then finish it | Exit with it live: identify the acquisition, remaining obligation, and exact exit. |
| Return the owner or transfer it to a checked helper | Empty helper: report its owned parameter and unresolved function exit. |
| Resolve on both branches, or return remaining ownership in a sum | Resolve only on one path: show the unresolved predecessor, not merely “unused value.” |
| Retain an outer owner through a retry loop; finish iteration-local owners | `continue` leaks a local or moves an outer owner without replacement. |
| Exchange an owner and resolve the returned old value | Overwrite, ignored temporary, or `drop`: identify the implicit/explicit discard. |
| Pass the same seal through a valid state transition | Reuse after move, fabricate a state, or discard a deconstructed seal. |

The companion pages add error, aggregate, scope, and worker cases. Native tests
must inject allocation and submission failures and count transfers/terminal
calls, including cancelled work, so every acquired seal is either returned or
resolved once. Compile-fail tests must cover implicit cleanup as well as source
operations. These are future conformance requirements, not tests run for this
proposal.

## Alternatives and open decisions

An expanded unused-value warning is cheaper but cannot enforce obligations
across wrappers and calls. Affine RAII remains preferable when silent fallback
cleanup is an adequate resource contract. Requiring all owners to be linear
would impose unnecessary handling on ordinary containers and existing programs.
Typestate alone and a combined effects redesign are assessed in the
[integration contract](linear-integration.md).

Proposed decisions are opt-in linearity, recursive non-discardability, checked
transfer, and explicit terminal authority. Open design choices are declaration
and capability syntax, how trusted contracts may be extended, the smallest
consuming-deconstruction surface, and how much path-sensitive empty-container
proof to expose. None may weaken error-path or aggregate preservation. Async
completion and general effect polymorphism are explicitly deferred.
